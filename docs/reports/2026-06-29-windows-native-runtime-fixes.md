# Windows Native Runtime Fixes

Date: 2026-06-29
Version: 0.1.35

## Context

Native Windows smoke testing of `v0.1.34` showed three problems:

- `.\tinytop.ps1` could be blocked by PowerShell execution policy.
- `.\tinytop.ps1 service install` failed under `Set-StrictMode` when the single service subcommand was treated as a scalar instead of an array.
- Direct `tinytop-agent.exe serve` failed with `environment variable not found` because the Rust default SQLite path required Unix-style `HOME`.

The same smoke run also showed a product issue: native Windows and WSL/Linux both used `127.0.0.1:4274`, making it easy to open the wrong daemon on shared loopback.

## Changes

- Added `tinytop.cmd`, which launches `tinytop.ps1` through `powershell.exe -ExecutionPolicy Bypass` for only that process.
- Documented `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass` as the direct `.ps1` workaround.
- Changed native Windows dashboard default port to `127.0.0.1:4275`.
- Added Windows command-center detection for another TinyTop daemon on the WSL/Linux default port `127.0.0.1:4274`.
- Fixed PowerShell rest-argument handling with `@($args[1..($args.Count - 1)])`.
- Added Windows default SQLite resolution in Rust: `%LOCALAPPDATA%\TinyTop\state\history.sqlite`, falling back to `%USERPROFILE%\AppData\Local\TinyTop\state\history.sqlite`.
- Added daemon OS, architecture, executable path, working directory, bind host/port, and SQLite URL/path metadata to `/health` and `/api/version`.
- Added a dashboard runtime-origin notice for native Windows versus WSL/Linux daemon confusion.

## Operator Notes

On native Windows:

```powershell
.\tinytop.cmd rust install-binary
.\tinytop.cmd start
```

Or, for direct PowerShell script use:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\tinytop.ps1 rust install-binary
.\tinytop.ps1 start
```

Expected native Windows URL:

```text
http://127.0.0.1:4275
```

Expected WSL/Linux URL:

```text
http://127.0.0.1:4274
```

Both `/health` and `/api/version` now identify the daemon and SQLite location.

## Verification

```bash
bun run check
```

Result:

- Bun: 92 tests passed.
- Rust: `cargo fmt --check` passed.
- Rust workspace: 29 tests passed across agent, collector, store, and contract suites.

```bash
./tinytop rust build
```

Result: built `agent/target/release/tinytop-agent` with package version `0.1.35`.

Temporary release-binary smoke:

```bash
./agent/target/release/tinytop-agent serve --host 127.0.0.1 --port 4285 --sqlite sqlite::memory: --poll-ms 100000 --no-dashboard
curl -fsS http://127.0.0.1:4285/health
curl -fsS http://127.0.0.1:4285/api/version
```

Result: both endpoints returned `version: 0.1.35`, `daemon.os: linux`, `daemon.bind.port: 4285`, and `daemon.storage.sqlitePath: memory`.

```bash
cargo audit
bun audit
```

Result: both exited successfully; `bun audit` reported no vulnerabilities.
