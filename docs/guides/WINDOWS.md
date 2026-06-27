# Windows Guide

TinyTop now has a native PowerShell command center for the Rust collector/dashboard daemon. Windows support is still a first native slice: the collector module exists, the PowerShell lifecycle exists, and the service commands exist, but release packaging and live Windows host parity still need a real Windows `.exe` asset and host verification.

## Current Support

- Entry point: `.\tinytop.ps1`
- Runtime: Rust `tinytop-agent.exe`
- Dashboard: `http://127.0.0.1:4274`
- Local install path: `%LOCALAPPDATA%\TinyTop\bin\tinytop-agent.exe`
- State path: `%LOCALAPPDATA%\TinyTop\state`
- Log paths: `%LOCALAPPDATA%\TinyTop\logs\tinytop.log` and `%LOCALAPPDATA%\TinyTop\logs\tinytop.err.log`
- SQLite path: `%LOCALAPPDATA%\TinyTop\state\history.sqlite` unless `TINYTOP_HISTORY_DB` is set
- Service path: Windows Service Control Manager through `.\tinytop.ps1 service ...`

## Install From Release Asset

When a Windows release asset exists, install it with:

```powershell
.\tinytop.ps1 rust install-binary
```

The expected release asset name is:

```text
tinytop-agent-windows-x86_64.exe
```

If the release does not contain that asset yet, the command fails with a local compile fallback.

## Build Locally

Install Rust from <https://rustup.rs>, then run:

```powershell
.\tinytop.ps1 rust build
```

The PowerShell command center builds with the Windows collector feature:

```powershell
cargo build --release --manifest-path agent/Cargo.toml -p tinytop-agent --no-default-features --features windows-collector
```

## Run Foreground Or Background Process

Start:

```powershell
.\tinytop.ps1 start
```

Status:

```powershell
.\tinytop.ps1 status
```

Logs:

```powershell
.\tinytop.ps1 logs
```

Stop:

```powershell
.\tinytop.ps1 stop
```

## Windows Service

TinyTop can install a real Windows service. Install and uninstall require an elevated PowerShell session:

```powershell
.\tinytop.ps1 service install
.\tinytop.ps1 service start
.\tinytop.ps1 service status
.\tinytop.ps1 service stop
.\tinytop.ps1 service uninstall
```

The command center checks elevation before every mutating service action: `install`, `start`, `stop`, `restart`, and `uninstall`. If PowerShell is not elevated in an interactive session, TinyTop warns and asks for explicit confirmation before attempting the service action. Non-interactive non-elevated runs fail with Administrator guidance. `service status` remains read-only and does not prompt.

The service runs `tinytop-agent.exe serve --host 127.0.0.1 --port 4274 --sqlite <path>`. The explicit SQLite path keeps service storage under the TinyTop local state directory instead of relying on a process profile's default home directory.

## Package Manager Roadmap

PowerShell is the bootstrap layer. Package managers come after real Windows release assets:

1. Scoop manifest pointing at GitHub release assets.
2. winget manifest after the Windows asset and service behavior are stable.
3. MSI/MSIX when code signing, Start Menu integration, and uninstall behavior are worth the added maintenance.

## Verification

Linux-side checks that do not spend GitHub Actions minutes:

```bash
bun test tests/tinytop-powershell.test.ts
cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector
```

Remaining Windows-host checks:

- build `tinytop-agent.exe` on Windows
- run `.\tinytop.ps1 start`
- verify `http://127.0.0.1:4274/health`
- verify `http://127.0.0.1:4274/api/version`
- install/start/stop/uninstall the Windows service from elevated PowerShell
