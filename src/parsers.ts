export type MemorySnapshot = {
  totalBytes: number;
  freeBytes: number;
  availableBytes: number;
  buffersBytes: number;
  cachedBytes: number;
  usedBytes: number;
  usedPercent: number;
  swapTotalBytes: number;
  swapFreeBytes: number;
  swapUsedBytes: number;
  swapUsedPercent: number;
};

export type LoadAverage = {
  one: number;
  five: number;
  fifteen: number;
  runnable: number;
  totalThreads: number;
  lastPid: number;
};

export type CpuTimes = {
  user: number;
  nice: number;
  system: number;
  idle: number;
  iowait: number;
  irq: number;
  softirq: number;
  steal: number;
  guest: number;
  guestNice: number;
  total: number;
  idleTotal: number;
};

export type PressureLine = {
  avg10: number;
  avg60: number;
  avg300: number;
  total: number;
};

export type PressureSnapshot = {
  some?: PressureLine;
  full?: PressureLine;
};

export type FilesystemBlocks = {
  filesystem: string;
  type: string;
  sizeBytes: number;
  usedBytes: number;
  availableBytes: number;
  usedPercent: number;
  mount: string;
};

export type RuntimeDetectionInput = {
  osRelease: string;
  procVersion: string;
  wslDistroName: string | undefined;
  wslInterop: string | undefined;
};

export type RuntimeDetection = {
  kind: "WSL" | "Linux" | "Unknown";
  confidence: "high" | "medium" | "low";
  reason: string;
};

const KIB = 1024;

function roundPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value * 10) / 10));
}

function toNumber(value: string | undefined): number {
  const parsed = Number(value ?? "0");
  return Number.isFinite(parsed) ? parsed : 0;
}

export function parseMeminfo(text: string): MemorySnapshot {
  const values = new Map<string, number>();

  for (const line of text.split("\n")) {
    const match = line.match(/^([A-Za-z_()]+):\s+(\d+)\s+kB$/);
    if (match) values.set(match[1], Number(match[2]) * KIB);
  }

  const totalBytes = values.get("MemTotal") ?? 0;
  const freeBytes = values.get("MemFree") ?? 0;
  const availableBytes = values.get("MemAvailable") ?? freeBytes;
  const buffersBytes = values.get("Buffers") ?? 0;
  const cachedBytes = values.get("Cached") ?? 0;
  const usedBytes = Math.max(0, totalBytes - availableBytes);
  const usedPercent = totalBytes === 0 ? 0 : roundPercent((usedBytes / totalBytes) * 100);
  const swapTotalBytes = values.get("SwapTotal") ?? 0;
  const swapFreeBytes = values.get("SwapFree") ?? 0;
  const swapUsedBytes = Math.max(0, swapTotalBytes - swapFreeBytes);
  const swapUsedPercent =
    swapTotalBytes === 0 ? 0 : roundPercent((swapUsedBytes / swapTotalBytes) * 100);

  return {
    totalBytes,
    freeBytes,
    availableBytes,
    buffersBytes,
    cachedBytes,
    usedBytes,
    usedPercent,
    swapTotalBytes,
    swapFreeBytes,
    swapUsedBytes,
    swapUsedPercent,
  };
}

export function parseLoadavg(text: string): LoadAverage {
  const [one, five, fifteen, threadCounts, lastPid] = text.trim().split(/\s+/);
  const [runnable, totalThreads] = (threadCounts ?? "0/0").split("/");

  return {
    one: toNumber(one),
    five: toNumber(five),
    fifteen: toNumber(fifteen),
    runnable: toNumber(runnable),
    totalThreads: toNumber(totalThreads),
    lastPid: toNumber(lastPid),
  };
}

export function parseProcStat(text: string): CpuTimes {
  const line = text
    .split("\n")
    .find((candidate) => candidate.startsWith("cpu "));
  if (!line) {
    throw new Error("Unable to find aggregate cpu line in /proc/stat");
  }

  const [
    user = 0,
    nice = 0,
    system = 0,
    idle = 0,
    iowait = 0,
    irq = 0,
    softirq = 0,
    steal = 0,
    guest = 0,
    guestNice = 0,
  ] = line
    .trim()
    .split(/\s+/)
    .slice(1)
    .map(Number);
  const idleTotal = idle + iowait;
  const total = user + nice + system + idle + iowait + irq + softirq + steal + guest + guestNice;

  return {
    user,
    nice,
    system,
    idle,
    iowait,
    irq,
    softirq,
    steal,
    guest,
    guestNice,
    total,
    idleTotal,
  };
}

export function calculateCpuUsage(previous: CpuTimes, current: CpuTimes): number {
  const totalDelta = current.total - previous.total;
  const idleDelta = current.idleTotal - previous.idleTotal;
  if (totalDelta <= 0) return 0;
  return roundPercent(((totalDelta - idleDelta) / totalDelta) * 100);
}

export function parsePressure(text: string): PressureSnapshot {
  const snapshot: PressureSnapshot = {};

  for (const line of text.trim().split("\n")) {
    const [label, ...pairs] = line.trim().split(/\s+/);
    if (label !== "some" && label !== "full") continue;

    const values = new Map<string, number>();
    for (const pair of pairs) {
      const [key, value] = pair.split("=");
      values.set(key, toNumber(value));
    }

    snapshot[label] = {
      avg10: values.get("avg10") ?? 0,
      avg60: values.get("avg60") ?? 0,
      avg300: values.get("avg300") ?? 0,
      total: values.get("total") ?? 0,
    };
  }

  return snapshot;
}

export function parseDfBlocks(text: string): FilesystemBlocks[] {
  return text
    .trim()
    .split("\n")
    .slice(1)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const parts = line.split(/\s+/);
      const [filesystem, type, size, used, available, percent, ...mountParts] = parts;
      return {
        filesystem,
        type,
        sizeBytes: toNumber(size),
        usedBytes: toNumber(used),
        availableBytes: toNumber(available),
        usedPercent: toNumber(percent?.replace("%", "")),
        mount: mountParts.join(" "),
      };
    });
}

export function detectLinuxRuntime(input: RuntimeDetectionInput): RuntimeDetection {
  const release = input.osRelease.toLowerCase();
  const version = input.procVersion.toLowerCase();

  if (
    release.includes("microsoft") ||
    release.includes("wsl") ||
    version.includes("microsoft") ||
    version.includes("wsl")
  ) {
    return {
      kind: "WSL",
      confidence: "high",
      reason: "kernel release/version contains Microsoft WSL markers",
    };
  }

  if (input.wslDistroName || input.wslInterop) {
    return {
      kind: "WSL",
      confidence: "medium",
      reason: "WSL environment variables are present",
    };
  }

  if (release || version) {
    return {
      kind: "Linux",
      confidence: "high",
      reason: "no WSL kernel or environment markers detected",
    };
  }

  return {
    kind: "Unknown",
    confidence: "low",
    reason: "kernel release and version were unavailable",
  };
}
