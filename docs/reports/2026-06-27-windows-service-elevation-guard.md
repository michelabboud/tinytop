# Windows Service Elevation Guard

Date: 2026-06-27
Version: 0.1.33

## Summary

TinyTop's Windows PowerShell command center now checks elevation before mutating Windows Service Control Manager state.

The guard applies to:

- `.\tinytop.ps1 service install`
- `.\tinytop.ps1 service start`
- `.\tinytop.ps1 service stop`
- `.\tinytop.ps1 service restart`
- `.\tinytop.ps1 service uninstall`

`.\tinytop.ps1 service status` remains read-only and does not prompt.

## Behavior

- Elevated PowerShell sessions run service mutations directly.
- Interactive non-elevated sessions show a warning and require an explicit `y` or `yes` confirmation before attempting the service action.
- Non-interactive non-elevated sessions fail fast with guidance to rerun PowerShell as Administrator or rerun interactively to confirm.

## Verification

- Added regression coverage in `tests/tinytop-powershell.test.ts` for the shared elevation/confirmation helper and every mutating service action.
- The targeted PowerShell command-center test was first run in red state and failed because the helper did not exist.
- The targeted PowerShell command-center test passed after the guard was implemented: `5 pass`, `0 fail`.
- `./tinytop rust build`: rebuilt the embedded Rust agent as `0.1.33`.
- `./tinytop status`: confirmed the running daemon reports `rust collector-dashboard-daemon v0.1.33 (embedded dashboard)`.
- The README screenshot was refreshed from the running `0.1.33` dashboard.
- `bun run check`: passed with `85 pass`, `0 fail`.
- `git diff --check`: passed.
- `bun audit`: no vulnerabilities found.
- `cargo audit`: scanned 196 crate dependencies with no vulnerabilities reported.
- Release asset checksum: `90473c4c3d96afea998089f503713d724ed12becd2b8d308ff8eb1e758da0b99`.
