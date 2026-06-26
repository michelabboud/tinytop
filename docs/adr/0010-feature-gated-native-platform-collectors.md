# 0010 Feature-Gated Native Platform Collectors

## Context

Linux/WSL is TinyTop's reference collector because the current dashboard depends on Linux details such as `/proc` CPU ticks, pressure stall information, and load thread counts. macOS and Windows support should start without weakening Linux or pretending full parity exists.

The Rust workspace already uses `sysinfo`, which can provide portable identity, CPU, memory, disk, load-average-equivalent, and process data across platforms.

## Decision

Keep Linux as the default collector feature and introduce opt-in native platform modules:

- `linux-collector` as the default feature
- `macos-collector` behind `target_os = "macos"`
- `windows-collector` behind `target_os = "windows"`

Expose a `NativeCollector` alias selected by target and feature. macOS and Windows use a shared `sysinfo` collector core for the first native slice: identity, CPU, memory/swap, load equivalent, disks, and top processes with parent PID/start time when available. Linux remains the reference implementation and keeps `procfs` plus `sysinfo`.

## Alternatives Rejected

- Make cross-platform collectors default immediately. Rejected because macOS/Windows live-host parity and packaging are not verified yet.
- Hide platform gaps behind fake Linux-shaped values. Rejected because it would make the dashboard less trustworthy.
- Add platform-specific dependencies now. Rejected because `sysinfo` already covers the first useful slice without expanding the dependency surface.

## Consequences

Linux builds remain unchanged for normal users. macOS and Windows collectors can be checked and iterated behind explicit build features. Full daemon cross-compilation may still require platform toolchains for SQLite, independent of the collector module itself.

## Status

Accepted.
