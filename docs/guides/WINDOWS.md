# Windows Guide

TinyTop has a native Windows command center for the Rust collector/dashboard daemon. Windows support includes release `.exe` installation, local Rust builds, foreground lifecycle commands, Windows service commands, and runtime metadata that identifies whether the browser is connected to native Windows or WSL/Linux.

## Current Support

- Entry point: `.\tinytop.cmd` or `.\tinytop.ps1`
- Runtime: Rust `tinytop-agent.exe`
- Dashboard: `http://127.0.0.1:4275` by default on native Windows
- WSL/Linux dashboard default: `http://127.0.0.1:4274`
- Local install path: `%LOCALAPPDATA%\TinyTop\bin\tinytop-agent.exe`
- State path: `%LOCALAPPDATA%\TinyTop\state`
- Log paths: `%LOCALAPPDATA%\TinyTop\logs\tinytop.log` and `%LOCALAPPDATA%\TinyTop\logs\tinytop.err.log`
- SQLite path: `%LOCALAPPDATA%\TinyTop\state\history.sqlite` unless `TINYTOP_HISTORY_DB` is set
- Service path: Windows Service Control Manager through `.\tinytop.cmd service ...` or `.\tinytop.ps1 service ...`

## Install From Release Asset

When a Windows release asset exists, install it with:

```powershell
.\tinytop.cmd rust install-binary
```

The expected release asset name is:

```text
tinytop-agent-windows-x86_64.exe
```

If the release does not contain that asset yet, the command fails with a local compile fallback.

## Build Locally

Install Rust from <https://rustup.rs>, then run:

```powershell
.\tinytop.cmd rust build
```

The PowerShell command center builds with the Windows collector feature:

```powershell
cargo build --release --manifest-path agent/Cargo.toml -p tinytop-agent --no-default-features --features windows-collector
```

## Run Foreground Or Background Process

Start:

```powershell
.\tinytop.cmd start
```

Status:

```powershell
.\tinytop.cmd status
```

Logs:

```powershell
.\tinytop.cmd logs
```

Stop:

```powershell
.\tinytop.cmd stop
```

## Windows Service

TinyTop can install a real Windows service. Install and uninstall require an elevated PowerShell session:

```powershell
.\tinytop.cmd service install
.\tinytop.cmd service start
.\tinytop.cmd service status
.\tinytop.cmd service stop
.\tinytop.cmd service uninstall
```

The command center checks elevation before every mutating service action: `install`, `start`, `stop`, `restart`, and `uninstall`. If PowerShell is not elevated in an interactive session, TinyTop warns and asks for explicit confirmation before attempting the service action. Non-interactive non-elevated runs fail with Administrator guidance. `service status` remains read-only and does not prompt.

The service runs `tinytop-agent.exe serve --host 127.0.0.1 --port 4275 --sqlite <path>`. The explicit SQLite path keeps service storage under the TinyTop local state directory instead of relying on a process profile's default home directory.

## Windows And WSL Running Together

Native Windows defaults to `127.0.0.1:4275` so it does not collide with a WSL/Linux TinyTop daemon on `127.0.0.1:4274`. The Windows command center checks the WSL/Linux default port and warns when another TinyTop daemon is already visible there.

The dashboard also shows a runtime-origin notice when it is served by native Windows or by WSL/Linux. `GET /health` and `GET /api/version` include:

- daemon OS and architecture
- daemon executable path and working directory
- bind host and port
- SQLite URL/path

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
- run `.\tinytop.cmd start`
- verify `http://127.0.0.1:4275/health`
- verify `http://127.0.0.1:4275/api/version`
- install/start/stop/uninstall the Windows service from elevated PowerShell
If you call `tinytop.ps1` directly and PowerShell scripts are disabled, use a process-scoped bypass for only the current shell:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\tinytop.ps1 rust install-binary
```

The `.\tinytop.cmd` wrapper does this for the launched PowerShell process without changing user or machine policy.
