import { describe, expect, test } from "bun:test";
import { buildSnapshotFromSources } from "../src/collector";

const baseSources = {
  timestamp: "2026-06-24T12:00:00.000Z",
  hostname: "devbox",
  platform: "linux",
  arch: "x64",
  cpuCount: 4,
  osRelease: "5.15.167.4-microsoft-standard-WSL2",
  procVersion: "Linux version 5.15.167.4-microsoft-standard-WSL2",
  kernelRelease: "5.15.167.4-microsoft-standard-WSL2",
  distroName: "Ubuntu 24.04.2 LTS",
  wslDistroName: "Ubuntu-24.04",
  wslInterop: "/run/WSL/123_interop",
  uptimeSeconds: 3661,
  meminfoText: `
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
Cached:          3072000 kB
SwapTotal:       4194304 kB
SwapFree:        1048576 kB
`,
  loadavgText: "1.20 2.30 3.40 5/678 9012",
  previousProcStatText: "cpu  100 0 100 800 0 0 0 0 0 0",
  currentProcStatText: "cpu  150 0 150 900 0 0 0 0 0 0",
  cpuPressureText: "some avg10=2.00 avg60=1.00 avg300=0.50 total=100\n",
  memoryPressureText:
    "some avg10=3.00 avg60=2.00 avg300=1.00 total=200\nfull avg10=0.30 avg60=0.20 avg300=0.10 total=20\n",
  ioPressureText:
    "some avg10=4.00 avg60=3.00 avg300=2.00 total=300\nfull avg10=0.40 avg60=0.30 avg300=0.20 total=30\n",
  dfBlocksText: `Filesystem     Type  1-blocks     Used Available Use% Mounted on
/dev/sdd       ext4  1000 800 200 80% /
tmpfs          tmpfs 500 25 475 5% /run
`,
  dfInodesText: `Filesystem     Type  Inodes IUsed IFree IUse% Mounted on
/dev/sdd       ext4  100 20 80 20% /
tmpfs          tmpfs 50 1 49 2% /run
`,
  processes: [
    { pid: 101, command: "bun", cpuPercent: 12.5, memoryPercent: 1.2, rssBytes: 123_000_000 },
    { pid: 202, command: "postgres", cpuPercent: 3.1, memoryPercent: 5.4, rssBytes: 456_000_000 },
  ],
};

describe("buildSnapshotFromSources", () => {
  test("builds the dashboard JSON contract from raw WSL/Linux sources", () => {
    const snapshot = buildSnapshotFromSources(baseSources);

    expect(snapshot.identity).toEqual({
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
      uptimeSeconds: 3661,
    });
    expect(snapshot.cpu.usagePercent).toBe(50);
    expect(snapshot.cpu.cores).toBe(4);
    expect(snapshot.memory.usedPercent).toBe(50);
    expect(snapshot.swap.usedPercent).toBe(75);
    expect(snapshot.load.one).toBe(1.2);
    expect(snapshot.filesystems[0]).toMatchObject({
      mount: "/",
      filesystem: "/dev/sdd",
      type: "ext4",
      usedPercent: 80,
      inodeUsedPercent: 20,
    });
    expect(snapshot.pressure.memory.full?.avg10).toBe(0.3);
    expect(snapshot.processes[0]?.command).toBe("bun");
  });

  test("classifies real Linux when WSL markers are absent", () => {
    const snapshot = buildSnapshotFromSources({
      ...baseSources,
      osRelease: "6.8.0-52-generic",
      procVersion: "Linux version 6.8.0-52-generic",
      kernelRelease: "6.8.0-52-generic",
      wslDistroName: undefined,
      wslInterop: undefined,
    });

    expect(snapshot.identity.runtime.kind).toBe("Linux");
    expect(snapshot.identity.runtime.reason).toBe("no WSL kernel or environment markers detected");
  });
});
