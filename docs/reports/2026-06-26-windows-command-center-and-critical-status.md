# Windows Command Center And Critical Status Report

Date: 2026-06-26

## Scope

This checkpoint adds a native Windows control layer for the Rust collector/dashboard daemon, makes the dashboard Critical operator state more visible, and tidies the sidebar runtime identity.

## What Changed

- Added `tinytop.ps1` with Windows-native help, doctor/status, release binary install, local Rust build, start, stop, restart, logs, and service commands.
- Added explicit Windows collector build selection through `--no-default-features --features windows-collector`.
- Updated the Bash command center so Windows-like shells use `tinytop-agent.exe` and can print target-specific Cargo build commands.
- Added Windows service commands through PowerShell and Windows Service Control Manager. Service install/uninstall require Administrator.
- Strengthened the operator status strip styling for Healthy, Warning, Critical, and Stale states. Critical now changes the whole strip, its cells, and the state pill instead of relying on a subtle border.
- Reworked the sidebar runtime text so long WSL detection reasons collapse into a compact runtime pill with hover detail.
- Added ADR 0011 for the PowerShell-first Windows packaging decision.

## Service Answer

Yes, TinyTop now has a Windows service path in `tinytop.ps1`:

```powershell
.\tinytop.ps1 service install
.\tinytop.ps1 service start
.\tinytop.ps1 service status
.\tinytop.ps1 service stop
.\tinytop.ps1 service uninstall
```

Install and uninstall require an elevated PowerShell session. Foreground/background `start`, `stop`, `status`, and `logs` do not require service installation.

## Current Limitation

The current public release may not yet include `tinytop-agent-windows-x86_64.exe`. Until that asset exists, Windows users should compile locally with Rust or receive a manually built `.exe`.

## Verification

Focused checks:

```bash
bun test tests/tinytop-powershell.test.ts tests/tinytop-script.test.ts tests/dashboard-operator-alert.test.ts tests/dashboard-assets.test.ts
shellcheck tinytop
cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector
diff -qr agent/assets/dashboard legacy/dashboard
bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts tests/dashboard-operator-alert.test.ts
```

Results:

- PowerShell/static, Bash command-center, operator alert, and dashboard asset tests: `28 pass`, `0 fail`.
- `shellcheck tinytop`: clean.
- Windows collector crate cross-target check: passed.
- Dashboard asset parity: no differences.
- Sidebar runtime identity regression coverage: passed.

Full checks:

```bash
./tinytop check
cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings
git diff --check
bun audit
cargo audit --file agent/Cargo.lock
./tinytop rust build
```

Results:

- `./tinytop check`: Bun tests `85 pass`, `0 fail`; Rust workspace tests passed.
- `cargo clippy`: clean.
- `git diff --check`: clean.
- `bun audit`: no vulnerabilities found.
- `cargo audit`: scanned `196` crate dependencies with exit code `0`.
- `./tinytop rust build`: built the v0.1.29 release binary.

Live embedded dashboard smoke:

- `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.29 (embedded dashboard)`.
- `/health`: returned `ok`.
- `/api/version`: returned version `0.1.29`.
- Rendered browser smoke confirmed the sidebar runtime summary is compact (`WSL`) and the full WSL reason is clamped with hover/title detail.
