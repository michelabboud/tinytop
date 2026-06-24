# Rust Daemon Dependency Vetting

## Summary

TinyTop `0.1.13` adds a Rust daemon path that can serve the dashboard, own
SQLite, collect Linux/WSL metrics, and expose dashboard plus writer-compatible
HTTP APIs without Bun at runtime.

## Axum

- Version pinned: `0.8.9`
- License: `MIT`
- Minimum Rust version: `1.80`
- Source checked: `cargo search axum`, `cargo info axum`, and current Axum docs
  through Context7 library `/tokio-rs/axum/axum_v0_8_4`
- Features enabled: `http1`, `json`, `original-uri`, `query`, `tokio`
- Default features disabled: yes
- Reason selected: Axum is the Tokio ecosystem's maintained HTTP routing layer
  and provides the exact small surface TinyTop needs: router state, query
  extraction, JSON responses, static route handling, `TcpListener` serving, and
  graceful shutdown.
- Alternatives considered: raw Hyper would reduce one abstraction layer but
  would add more custom routing/error code; keeping Bun for the dashboard would
  leave Bun as a runtime dependency; a bespoke HTTP parser is not appropriate
  for a public local daemon.

## Tokio Feature Expansion

- Version pinned: `1.52.3`
- Existing dependency: yes
- Features added: `net`, `signal`, `sync`, `time`
- Reason: the Rust daemon needs `TcpListener`, graceful Ctrl-C shutdown,
  async mutex state sharing, and interval-based scheduled collection.

## Apache ECharts Vendored Browser Asset

- Version: `6.1.0` from the existing npm dependency
- License: Apache License 2.0
- Source copied from: `node_modules/echarts/dist/echarts.min.js`
- Destination: `public/vendor/echarts.min.js`
- License/notice files copied:
  - `public/vendor/echarts.LICENSE`
  - `public/vendor/echarts.NOTICE`
  - `public/vendor/echarts.LICENSE-d3`
- Reason selected: the Rust no-Bun runtime must serve dashboard assets without
  `node_modules` or internet access at startup. Redistributing the already-used
  Apache ECharts browser bundle keeps the dashboard offline-capable.

## Security And Maintenance Notes

- The daemon binds to loopback by default.
- The HTTP surface is GET-only and returns JSON errors for API failures.
- Static file serving is restricted to the known dashboard asset paths.
- The Rust collector path still avoids shelling out for host metrics.
- Run `cargo audit` during release verification.
