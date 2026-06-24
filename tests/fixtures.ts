import type { SystemSnapshot } from "../src/collector";

export function makeSnapshot(overrides: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return {
    timestamp: "2026-06-24T12:00:00.000Z",
    identity: {
      hostname: "devbox",
      platform: "linux",
      arch: "x64",
      distro: "Ubuntu 24.04.2 LTS",
      kernel: "5.15.167.4-microsoft-standard-WSL2",
      runtime: {
        kind: "WSL",
        confidence: "high",
        reason: "kernel release/version contains Microsoft WSL markers",
      },
      uptimeSeconds: 123,
    },
    cpu: {
      usagePercent: 42,
      cores: 8,
      times: {
        user: 1,
        nice: 0,
        system: 1,
        idle: 8,
        iowait: 0,
        irq: 0,
        softirq: 0,
        steal: 0,
        guest: 0,
        guestNice: 0,
        total: 10,
        idleTotal: 8,
      },
    },
    memory: {
      totalBytes: 100,
      availableBytes: 40,
      usedBytes: 60,
      usedPercent: 60,
    },
    swap: {
      totalBytes: 100,
      freeBytes: 75,
      usedBytes: 25,
      usedPercent: 25,
    },
    load: {
      one: 1,
      five: 2,
      fifteen: 3,
      runnable: 1,
      totalThreads: 100,
      lastPid: 999,
    },
    pressure: {
      cpu: {},
      memory: {},
      io: {},
    },
    filesystems: [],
    processes: [],
    ...overrides,
  };
}
