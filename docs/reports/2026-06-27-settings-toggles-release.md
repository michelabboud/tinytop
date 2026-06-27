# Settings Toggles And v0.1.31 Release

Date: 2026-06-27

## Summary

TinyTop `0.1.31` updates the Settings dialog presentation, refreshes the README screenshot, and ships a rebuilt embedded Rust collector/dashboard daemon.

## Changes

- Fixed the effective-settings readout so compact summary chips stay top-aligned and do not stretch into oversized ovals beside the taller daemon settings group.
- Changed daemon boolean settings, including redaction and enabled dashboard sections, from tall single-column checkboxes into compact responsive toggle controls.
- Kept the existing checkbox inputs, IDs, labels, and settings persistence path intact.
- Kept `agent/assets/dashboard/styles.css` and `legacy/dashboard/styles.css` aligned for the shared dashboard asset invariant.
- Added `docs/assets/tinytop-dashboard-v0.1.31.png` and linked it from `README.md`.
- Bumped product, command-center, PowerShell, Cargo package, and lockfile metadata to `0.1.31`.

## Verification Results

- `bun run check`: passed with `85 pass`, `0 fail`, plus Rust fmt and workspace tests passing.
- `git diff --check`: passed with no whitespace errors.
- `./tinytop rust build`: passed and built `/home/michel/projects/tinytop/agent/target/release/tinytop-agent`.
- `curl -fsS http://127.0.0.1:4274/health`: returned `ok`.
- `curl -fsS http://127.0.0.1:4274/api/version`: returned Rust embedded dashboard identity for version `0.1.31`.
- `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.31 (embedded dashboard)` with dashboard port `4274` listening and legacy collector port `4276` free.
- `bun audit`: reported no vulnerabilities.
- `cargo audit` from `agent/`: scanned `Cargo.lock` successfully and exited cleanly after loading 1139 RustSec advisories.

## Release Assets

- Linux x86_64 binary: `dist/tinytop-agent-linux-x86_64`
- SHA-256: `571ffa2bd6a933da84b5a47fd1b7625f150a07a6dc7c517d6f8b9a473cc3d1cb`

## Runtime State After Release Build

The rebuilt foreground daemon was relaunched detached with:

```bash
setsid ./tinytop start > /tmp/tinytop-v0.1.31-toggles-20260627-100143.log 2>&1 < /dev/null &
```

The dashboard URL is `http://127.0.0.1:4274`.
