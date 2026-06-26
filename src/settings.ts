export type DashboardSettings = {
  defaultTheme: string;
  defaultGraphMode: string;
  pollIntervalMs: number;
  defaultHistoryWindow: string;
  retentionHours: number;
  rollupRetentionDays: number;
  topProcessCount: number;
  redactionDefault: boolean;
  thresholds: {
    cpuWarn: number;
    memoryWarn: number;
    diskWarn: number;
  };
  enabledSections: {
    overview: boolean;
    history: boolean;
    filesystem: boolean;
    pressure: boolean;
    processes: boolean;
  };
};

export const DEFAULT_DASHBOARD_SETTINGS: DashboardSettings = {
  defaultTheme: "midnight",
  defaultGraphMode: "line",
  pollIntervalMs: 1500,
  defaultHistoryWindow: "live",
  retentionHours: 72,
  rollupRetentionDays: 30,
  topProcessCount: 8,
  redactionDefault: false,
  thresholds: {
    cpuWarn: 80,
    memoryWarn: 85,
    diskWarn: 85,
  },
  enabledSections: {
    overview: true,
    history: true,
    filesystem: true,
    pressure: true,
    processes: true,
  },
};

const allowedThemes = new Set(["midnight", "matrix", "aurora", "solar", "ember"]);
const allowedGraphModes = new Set(["line", "area", "bar", "heatmap", "treemap"]);
const allowedHistoryWindows = new Set(["live", "15m", "1h", "6h", "24h"]);

export function cloneDashboardSettings(settings = DEFAULT_DASHBOARD_SETTINGS): DashboardSettings {
  return {
    ...settings,
    thresholds: { ...settings.thresholds },
    enabledSections: { ...settings.enabledSections },
  };
}

export function normalizeDashboardSettings(value: unknown): DashboardSettings {
  if (!value || typeof value !== "object") {
    throw new Error("settings payload must be an object");
  }

  const incoming = value as Partial<DashboardSettings>;
  const next = cloneDashboardSettings();
  next.defaultTheme = stringValue(incoming.defaultTheme, next.defaultTheme);
  next.defaultGraphMode = stringValue(incoming.defaultGraphMode, next.defaultGraphMode);
  next.defaultHistoryWindow = stringValue(incoming.defaultHistoryWindow, next.defaultHistoryWindow);
  next.pollIntervalMs = numberValue(incoming.pollIntervalMs, next.pollIntervalMs);
  next.retentionHours = numberValue(incoming.retentionHours, next.retentionHours);
  next.rollupRetentionDays = numberValue(incoming.rollupRetentionDays, next.rollupRetentionDays);
  next.topProcessCount = numberValue(incoming.topProcessCount, next.topProcessCount);
  next.redactionDefault = Boolean(incoming.redactionDefault);
  next.thresholds = {
    cpuWarn: numberValue(incoming.thresholds?.cpuWarn, next.thresholds.cpuWarn),
    memoryWarn: numberValue(incoming.thresholds?.memoryWarn, next.thresholds.memoryWarn),
    diskWarn: numberValue(incoming.thresholds?.diskWarn, next.thresholds.diskWarn),
  };
  next.enabledSections = {
    overview: booleanValue(incoming.enabledSections?.overview, next.enabledSections.overview),
    history: booleanValue(incoming.enabledSections?.history, next.enabledSections.history),
    filesystem: booleanValue(incoming.enabledSections?.filesystem, next.enabledSections.filesystem),
    pressure: booleanValue(incoming.enabledSections?.pressure, next.enabledSections.pressure),
    processes: booleanValue(incoming.enabledSections?.processes, next.enabledSections.processes),
  };

  validateDashboardSettings(next);
  return next;
}

function stringValue(value: unknown, fallback: string): string {
  return typeof value === "string" ? value : fallback;
}

function numberValue(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.round(value) : fallback;
}

function booleanValue(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function validateDashboardSettings(settings: DashboardSettings): void {
  validateOneOf("defaultTheme", settings.defaultTheme, allowedThemes);
  validateOneOf("defaultGraphMode", settings.defaultGraphMode, allowedGraphModes);
  validateOneOf("defaultHistoryWindow", settings.defaultHistoryWindow, allowedHistoryWindows);
  validateRange("pollIntervalMs", settings.pollIntervalMs, 250, 60_000);
  validateRange("retentionHours", settings.retentionHours, 1, 8_760);
  validateRange("rollupRetentionDays", settings.rollupRetentionDays, 1, 366);
  validateRange("topProcessCount", settings.topProcessCount, 1, 50);
  validateRange("thresholds.cpuWarn", settings.thresholds.cpuWarn, 0, 100);
  validateRange("thresholds.memoryWarn", settings.thresholds.memoryWarn, 0, 100);
  validateRange("thresholds.diskWarn", settings.thresholds.diskWarn, 0, 100);
}

function validateOneOf(field: string, value: string, allowed: Set<string>): void {
  if (!allowed.has(value)) {
    throw new Error(`${field} is invalid`);
  }
}

function validateRange(field: string, value: number, min: number, max: number): void {
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new Error(`${field} must be between ${min} and ${max}`);
  }
}
