# Live Connected README Screenshot

Date: 2026-06-27
Version: 0.1.32

## Summary

The README dashboard screenshot was replaced with a fresh capture from the running Rust collector/dashboard daemon on `127.0.0.1:4274`.

The new screenshot is intentionally captured after the dashboard hydrates, so it shows real connected values instead of an empty startup shell.

## Updated asset

- `docs/assets/tinytop-dashboard-v0.1.32.png`

## Visible evidence in the screenshot

- Runtime sidebar identity for TinyTop `v0.1.32`.
- Host, kernel, distro, and uptime values.
- Live operator state with recent sample age.
- CPU, RAM, swap, and load gauges with non-empty values.
- History section with populated sample count.
- Green `Live` connection indicator in the sidebar.

## Verification

- `./tinytop status`: confirmed the Rust collector/dashboard daemon is the active runtime.
- `curl -fsS http://127.0.0.1:4274/health`: returned `ok`.
- `curl -fsS http://127.0.0.1:4274/api/version`: returned the embedded Rust collector/dashboard identity.
- `curl -fsS http://127.0.0.1:4274/api/snapshot`: returned live host telemetry before the screenshot was captured.
- Headless Chrome wrote the screenshot after a 10 second virtual-time hydration budget.
- `bun run check`: passed.
- `bun audit`: no vulnerabilities found.
- `cargo audit`: scanned 196 crate dependencies with no vulnerabilities reported.
- Release asset checksum: `1e7a57eda312e607df14982fb79835a00b82ebd8cd8885c7254a554dd85519e7`.
