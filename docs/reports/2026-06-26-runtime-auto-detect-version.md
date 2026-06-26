# Runtime Auto-Detect And Version Identity

Date: 2026-06-26

## Summary

TinyTop now exposes explicit runtime/version identity instead of requiring users to infer old versus new runtime from ports or process names.

Implemented in `0.1.22`:

- Rust collector/dashboard daemon: `GET /api/version` and collector-compatible `GET /version`.
- Legacy Bun dashboard: `GET /api/version`.
- Legacy Bun collector: `GET /version`.
- Dashboard sidebar version line, for example `Rust collector/dashboard v0.1.22`.
- `./tinytop start` runtime auto-detection with Rust preferred when a release binary or Cargo is available.
- `TINYTOP_RUNTIME=rust`, `TINYTOP_RUNTIME=legacy`, and `TINYTOP_RUNTIME=bun` overrides.
- `./tinytop status` reads `GET /api/version` and reports the running daemon runtime, component, version, and dashboard asset mode.
- Foreground `./tinytop stop` and `./tinytop restart` detect Rust and legacy Bun runtimes when systemd units are not installed.

## Runtime Identity Response

Rust daemon:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.1.22",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded"
}
```

Legacy Bun dashboard:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.1.22",
  "runtime": "legacy-bun",
  "component": "dashboard",
  "dashboard": "legacy",
  "collector": {
    "status": "ok",
    "app": "tinytop",
    "version": "0.1.22",
    "runtime": "legacy-bun",
    "component": "collector",
    "dashboard": "none"
  }
}
```

## Operator Checks

```bash
./tinytop status
curl -fsS http://127.0.0.1:4274/api/version
```

Foreground runtime selection:

```bash
./tinytop start
TINYTOP_RUNTIME=rust ./tinytop start
TINYTOP_RUNTIME=legacy ./tinytop start
```

## Test Coverage

- `tests/tinytop-script.test.ts` covers Rust auto-selection, legacy override, and status version reporting.
- `tests/server.test.ts` covers legacy dashboard and collector version metadata.
- `tests/dashboard-timeline.test.ts` covers the dashboard version UI hook and `/api/version` fetch.
- `agent/crates/tinytop-agent/tests/serve_contract.rs` covers Rust daemon version identity.
