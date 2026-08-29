# 0022 - GPU telemetry from kernel/OS interfaces only, detection-gated

## Status

Accepted (2026-08-29 — Michel's go: "Go for the optimization plan fully") for plan `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md`. Extends ADR 0012 (no subprocess in collectors).

## Context

TinyTop collects no GPU metric. Michel's requirements (2026-08-29): GPU at the fast cadence like CPU/RAM/load; "if you do not identify a GPU then you do not collect its metrics"; "just use proc — I wouldn't want my daemon to run system calls for an exe file"; Windows and macOS need the equivalent right source. Measured sources: **Linux** — DRM sysfs (`/sys/class/drm/card*/device/`: `vendor`, `uevent` `DRIVER=`, `mem_info_vram_used/_total` on amdgpu, `gpu_busy_percent` where the ASIC supports it — trashcan's GCN-1 R7 370s answer `Operation not supported`; sheep/goat's Intel i915 have no busy file), `/proc/<pid>/fdinfo` DRM engine counters (kernel ≥ 5.19; sheep 6.8, trashcan 7.0), the GPU node's hwmon `temp1_input` (45 °C / 46 °C on trashcan). NVIDIA's proprietary driver exposes no utilization without NVML. **WSL2** — the guest kernel has no DRM device (`/sys/class/drm` = `version` only, `/dev/dxg` only); the Windows host's counters were reachable only by spawning `typeperf.exe` (2.4 s per one-shot sample, 2.04 s per streamed line). **Windows native** — PDH (`\GPU Engine(*)\Utilization Percentage`, `\GPU Adapter Memory(*)`; 3 LUIDs and 744 engine instances on WizAI on 2026-08-29; the `windows` crate is already in the lock at 0.62.2) and DXGI for identity/memory; vendor `0x1414` = Microsoft software adapters. **macOS** — IOKit `IOAccelerator` `PerformanceStatistics` (`Device Utilization %`, `In use system memory`, … on Apple silicon; `GPU Activity(%)` on others); `core-foundation` already in the lock.

## Decision

- A `GpuBackend` per OS, selected by **detection**: no real adapter → no sampler, no rows, no `gpus` section in the snapshot. Adapters are re-detected on the slow tick (eGPU hotplug).
- **Linux:** DRM sysfs + `/proc/<pid>/fdinfo` (busy % from `drm-engine-*` deltas per client, deduplicated by `drm-client-id`, max engine per client; per-process GPU % for the process table) + the GPU node's hwmon temperature. NVIDIA proprietary → identity only, `busyPercent: null`.
- **Windows native:** PDH + DXGI in-process (English counter names; the first sample after (re)open discarded; adapter busy = max over engine types of the per-process sum, Task Manager's definition; LUID joins PDH to DXGI; software adapters dropped).
- **macOS:** IOKit `IOAccelerator` → `PerformanceStatistics`, keys collected when present, `None` otherwise; hand-declared framework FFI, no new crate.
- **Never a subprocess, never a vendor library** (NVML/ADL/AGS). WSL2 collects nothing.
- Stored per tick in `gpu_samples` keyed by a stable adapter id (PCI slot on Linux, LUID on Windows, registry id on macOS) with a `gpu_adapters` dictionary; pruned at the L1 horizon. OTel GPU instruments are out of scope for this phase.

## Alternatives rejected

- **`typeperf.exe` / `nvidia-smi` / `powermetrics` subprocesses** — Michel's rule and ADR 0012; also 2+ s per sample from WSL2.
- **Vendor SDKs** — the daemon must not link or dlopen vendor code; NVIDIA on Linux therefore reports identity only.
- **Reading the Windows host from inside WSL2** — no in-process path exists; the box reports no GPU, honestly.
- **A PCI id → product name table** — maintenance weight; the sysfs `product_name` or the id pair suffices.

## Consequences

- GPU busy % and memory at 1.5 s where the OS exposes them; per-process GPU % on Linux and Windows; temperature on Linux only (the sensors plan `docs/plans/2026-08-16-sensors-recording-plan.md` owns fans/power/temps generally).
- Windows/macOS backends are compile-verified cross-target on the dev box and must be runtime-accepted on real hardware before the release notes claim them.
- Expected storage ≈ 30 B per adapter per tick (≈ 1.7 MB/day per adapter).
