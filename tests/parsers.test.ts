import { describe, expect, test } from "bun:test";
import {
  calculateCpuUsage,
  detectLinuxRuntime,
  parseDfBlocks,
  parseLoadavg,
  parseMeminfo,
  parsePressure,
  parseProcStat,
} from "../src/parsers";

describe("parseMeminfo", () => {
  test("normalizes memory and swap values from kB to bytes with used percentages", () => {
    const snapshot = parseMeminfo(`
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
Cached:          3072000 kB
SwapTotal:       4194304 kB
SwapFree:        1048576 kB
`);

    expect(snapshot.totalBytes).toBe(16_777_216_000);
    expect(snapshot.availableBytes).toBe(8_388_608_000);
    expect(snapshot.usedBytes).toBe(8_388_608_000);
    expect(snapshot.usedPercent).toBe(50);
    expect(snapshot.swapTotalBytes).toBe(4_294_967_296);
    expect(snapshot.swapUsedBytes).toBe(3_221_225_472);
    expect(snapshot.swapUsedPercent).toBe(75);
  });

  test("handles systems without swap without dividing by zero", () => {
    const snapshot = parseMeminfo(`
MemTotal:       1024000 kB
MemAvailable:   512000 kB
SwapTotal:           0 kB
SwapFree:            0 kB
`);

    expect(snapshot.usedPercent).toBe(50);
    expect(snapshot.swapUsedBytes).toBe(0);
    expect(snapshot.swapUsedPercent).toBe(0);
  });
});

describe("parseLoadavg", () => {
  test("parses load averages and runnable process counts", () => {
    expect(parseLoadavg("1.23 2.34 3.45 7/890 12345")).toEqual({
      one: 1.23,
      five: 2.34,
      fifteen: 3.45,
      runnable: 7,
      totalThreads: 890,
      lastPid: 12345,
    });
  });
});

describe("CPU stat parsing", () => {
  test("calculates CPU usage between two /proc/stat samples", () => {
    const previous = parseProcStat("cpu  100 0 100 800 0 0 0 0 0 0");
    const current = parseProcStat("cpu  150 0 150 900 0 0 0 0 0 0");

    expect(calculateCpuUsage(previous, current)).toBe(50);
  });
});

describe("parsePressure", () => {
  test("extracts avg10/avg60/avg300 and total for some/full lines", () => {
    expect(parsePressure(`some avg10=1.23 avg60=4.56 avg300=7.89 total=123456
full avg10=0.10 avg60=0.20 avg300=0.30 total=456
`)).toEqual({
      some: { avg10: 1.23, avg60: 4.56, avg300: 7.89, total: 123456 },
      full: { avg10: 0.1, avg60: 0.2, avg300: 0.3, total: 456 },
    });
  });
});

describe("parseDfBlocks", () => {
  test("parses POSIX df block output into filesystem records", () => {
    const filesystems = parseDfBlocks(`Filesystem     Type  1-blocks     Used Available Use% Mounted on
/dev/sdd       ext4  1081101176832 810825882624 215025180672  80% /
tmpfs          tmpfs 8589934592    4096 8589930496   1% /run
`);

    expect(filesystems).toEqual([
      {
        filesystem: "/dev/sdd",
        type: "ext4",
        sizeBytes: 1_081_101_176_832,
        usedBytes: 810_825_882_624,
        availableBytes: 215_025_180_672,
        usedPercent: 80,
        mount: "/",
      },
      {
        filesystem: "tmpfs",
        type: "tmpfs",
        sizeBytes: 8_589_934_592,
        usedBytes: 4_096,
        availableBytes: 8_589_930_496,
        usedPercent: 1,
        mount: "/run",
      },
    ]);
  });
});

describe("detectLinuxRuntime", () => {
  test("detects WSL from kernel release markers", () => {
    expect(
      detectLinuxRuntime({
        osRelease: "5.15.167.4-microsoft-standard-WSL2",
        procVersion: "Linux version 5.15.167.4-microsoft-standard-WSL2",
        wslDistroName: undefined,
        wslInterop: undefined,
      }),
    ).toEqual({
      kind: "WSL",
      confidence: "high",
      reason: "kernel release/version contains Microsoft WSL markers",
    });
  });

  test("detects WSL from environment markers when kernel markers are absent", () => {
    expect(
      detectLinuxRuntime({
        osRelease: "6.8.0-52-generic",
        procVersion: "Linux version 6.8.0-52-generic",
        wslDistroName: "Ubuntu-24.04",
        wslInterop: "/run/WSL/123_interop",
      }),
    ).toEqual({
      kind: "WSL",
      confidence: "medium",
      reason: "WSL environment variables are present",
    });
  });

  test("detects real Linux when no WSL markers are present", () => {
    expect(
      detectLinuxRuntime({
        osRelease: "6.8.0-52-generic",
        procVersion: "Linux version 6.8.0-52-generic",
        wslDistroName: undefined,
        wslInterop: undefined,
      }),
    ).toEqual({
      kind: "Linux",
      confidence: "high",
      reason: "no WSL kernel or environment markers detected",
    });
  });
});
