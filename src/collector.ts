import os from "node:os";
import {
  calculateCpuUsage,
  detectLinuxRuntime,
  parseDfBlocks,
  parseLoadavg,
  parseMeminfo,
  parsePressure,
  parseProcStat,
  type CpuTimes,
  type FilesystemBlocks,
  type LoadAverage,
  type PressureSnapshot,
  type RuntimeDetection,
} from "./parsers";

export type ProcessSnapshot = {
  pid: number;
  command: string;
  cpuPercent: number;
  memoryPercent: number;
  rssBytes: number;
};

export type SnapshotSources = {
  timestamp: string;
  hostname: string;
  platform: string;
  arch: string;
  cpuCount: number;
  osRelease: string;
  procVersion: string;
  kernelRelease: string;
  distroName: string;
  wslDistroName: string | undefined;
  wslInterop: string | undefined;
  uptimeSeconds: number;
  meminfoText: string;
  loadavgText: string;
  previousProcStatText: string;
  currentProcStatText: string;
  cpuPressureText: string;
  memoryPressureText: string;
  ioPressureText: string;
  dfBlocksText: string;
  dfInodesText: string;
  processes: ProcessSnapshot[];
};

export type FilesystemSnapshot = FilesystemBlocks & {
  inodeUsedPercent: number | null;
  inodeUsed: number | null;
  inodeTotal: number | null;
};

export type SystemSnapshot = {
  timestamp: string;
  identity: {
    hostname: string;
    platform: string;
    arch: string;
    distro: string;
    kernel: string;
    runtime: RuntimeDetection;
    uptimeSeconds: number;
  };
  cpu: {
    usagePercent: number;
    cores: number;
    times: CpuTimes;
  };
  memory: {
    totalBytes: number;
    availableBytes: number;
    usedBytes: number;
    usedPercent: number;
  };
  swap: {
    totalBytes: number;
    freeBytes: number;
    usedBytes: number;
    usedPercent: number;
  };
  load: LoadAverage;
  pressure: {
    cpu: PressureSnapshot;
    memory: PressureSnapshot;
    io: PressureSnapshot;
  };
  filesystems: FilesystemSnapshot[];
  processes: ProcessSnapshot[];
};

type InodeSnapshot = {
  mount: string;
  inodeTotal: number;
  inodeUsed: number;
  inodeUsedPercent: number;
};

const DEFAULT_TEXT = "";

function parseInodeDf(text: string): Map<string, InodeSnapshot> {
  const inodes = new Map<string, InodeSnapshot>();

  for (const line of text.trim().split("\n").slice(1)) {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 7) continue;

    const [, , inodeTotal, inodeUsed, , inodePercent, ...mountParts] = parts;
    const mount = mountParts.join(" ");
    inodes.set(mount, {
      mount,
      inodeTotal: Number(inodeTotal),
      inodeUsed: Number(inodeUsed),
      inodeUsedPercent: Number(inodePercent.replace("%", "")),
    });
  }

  return inodes;
}

function parsePrettyName(osReleaseText: string): string {
  const pretty = osReleaseText
    .split("\n")
    .find((line) => line.startsWith("PRETTY_NAME="))
    ?.replace("PRETTY_NAME=", "")
    .replace(/^"|"$/g, "");
  return pretty || "Linux";
}

function mergeFilesystems(blocks: FilesystemBlocks[], inodeText: string): FilesystemSnapshot[] {
  const inodes = parseInodeDf(inodeText);

  return blocks.map((filesystem) => {
    const inode = inodes.get(filesystem.mount);
    return {
      ...filesystem,
      inodeUsedPercent: inode?.inodeUsedPercent ?? null,
      inodeUsed: inode?.inodeUsed ?? null,
      inodeTotal: inode?.inodeTotal ?? null,
    };
  });
}

export function buildSnapshotFromSources(sources: SnapshotSources): SystemSnapshot {
  const memory = parseMeminfo(sources.meminfoText);
  const previousCpu = parseProcStat(sources.previousProcStatText);
  const currentCpu = parseProcStat(sources.currentProcStatText);
  const filesystems = mergeFilesystems(parseDfBlocks(sources.dfBlocksText), sources.dfInodesText);

  return {
    timestamp: sources.timestamp,
    identity: {
      hostname: sources.hostname,
      platform: sources.platform,
      arch: sources.arch,
      distro: sources.distroName,
      kernel: sources.kernelRelease,
      runtime: detectLinuxRuntime({
        osRelease: sources.osRelease,
        procVersion: sources.procVersion,
        wslDistroName: sources.wslDistroName,
        wslInterop: sources.wslInterop,
      }),
      uptimeSeconds: sources.uptimeSeconds,
    },
    cpu: {
      usagePercent: calculateCpuUsage(previousCpu, currentCpu),
      cores: sources.cpuCount,
      times: currentCpu,
    },
    memory: {
      totalBytes: memory.totalBytes,
      availableBytes: memory.availableBytes,
      usedBytes: memory.usedBytes,
      usedPercent: memory.usedPercent,
    },
    swap: {
      totalBytes: memory.swapTotalBytes,
      freeBytes: memory.swapFreeBytes,
      usedBytes: memory.swapUsedBytes,
      usedPercent: memory.swapUsedPercent,
    },
    load: parseLoadavg(sources.loadavgText),
    pressure: {
      cpu: parsePressure(sources.cpuPressureText),
      memory: parsePressure(sources.memoryPressureText),
      io: parsePressure(sources.ioPressureText),
    },
    filesystems,
    processes: sources.processes,
  };
}

async function readText(path: string, fallback = DEFAULT_TEXT): Promise<string> {
  try {
    return await Bun.file(path).text();
  } catch {
    return fallback;
  }
}

async function runText(command: string[], fallback = DEFAULT_TEXT): Promise<string> {
  try {
    const process = Bun.spawn(command, {
      stdout: "pipe",
      stderr: "pipe",
    });
    const [stdout, exitCode] = await Promise.all([
      new Response(process.stdout).text(),
      process.exited,
    ]);
    return exitCode === 0 ? stdout : fallback;
  } catch {
    return fallback;
  }
}

function parseUptime(text: string): number {
  const [seconds] = text.trim().split(/\s+/);
  const parsed = Number(seconds);
  return Number.isFinite(parsed) ? Math.floor(parsed) : 0;
}

function parseProcesses(text: string): ProcessSnapshot[] {
  return text
    .trim()
    .split("\n")
    .slice(0, 10)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [pid, cpu, memory, rss, ...commandParts] = line.split(/\s+/);
      return {
        pid: Number(pid),
        command: commandParts.join(" ") || "unknown",
        cpuPercent: Number(cpu),
        memoryPercent: Number(memory),
        rssBytes: Number(rss) * 1024,
      };
    })
    .filter((process) => Number.isFinite(process.pid));
}

export async function collectSources(previousProcStatText?: string): Promise<SnapshotSources> {
  const firstProcStat = previousProcStatText ?? (await readText("/proc/stat"));
  let currentProcStat = await readText("/proc/stat");

  if (!previousProcStatText) {
    await Bun.sleep(120);
    currentProcStat = await readText("/proc/stat");
  }

  const [
    osReleaseText,
    procVersion,
    kernelRelease,
    meminfoText,
    loadavgText,
    uptimeText,
    cpuPressureText,
    memoryPressureText,
    ioPressureText,
    dfBlocksText,
    dfInodesText,
    psText,
  ] = await Promise.all([
    readText("/etc/os-release"),
    readText("/proc/version"),
    runText(["uname", "-r"]),
    readText("/proc/meminfo"),
    readText("/proc/loadavg"),
    readText("/proc/uptime"),
    readText("/proc/pressure/cpu"),
    readText("/proc/pressure/memory"),
    readText("/proc/pressure/io"),
    runText(["df", "-PB1", "-T"]),
    runText(["df", "-Pi", "-T"]),
    runText(["ps", "-eo", "pid=,pcpu=,pmem=,rss=,comm=", "--sort=-pcpu"]),
  ]);

  return {
    timestamp: new Date().toISOString(),
    hostname: os.hostname(),
    platform: os.platform(),
    arch: os.arch(),
    cpuCount: os.cpus().length,
    osRelease: kernelRelease.trim(),
    procVersion,
    kernelRelease: kernelRelease.trim() || os.release(),
    distroName: parsePrettyName(osReleaseText),
    wslDistroName: process.env.WSL_DISTRO_NAME,
    wslInterop: process.env.WSL_INTEROP,
    uptimeSeconds: parseUptime(uptimeText),
    meminfoText,
    loadavgText,
    previousProcStatText: firstProcStat,
    currentProcStatText: currentProcStat,
    cpuPressureText,
    memoryPressureText,
    ioPressureText,
    dfBlocksText,
    dfInodesText,
    processes: parseProcesses(psText),
  };
}

export async function collectSnapshot(
  previousProcStatText?: string,
): Promise<{ snapshot: SystemSnapshot; currentProcStatText: string }> {
  const sources = await collectSources(previousProcStatText);
  return {
    snapshot: buildSnapshotFromSources(sources),
    currentProcStatText: sources.currentProcStatText,
  };
}
