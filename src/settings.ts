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
    cpuCritical: number;
    memoryWarn: number;
    memoryCritical: number;
    diskWarn: number;
    diskCritical: number;
    loadWarn: number;
    loadCritical: number;
    pressureWarn: number;
    pressureCritical: number;
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
    cpuCritical: 95,
    memoryWarn: 85,
    memoryCritical: 95,
    diskWarn: 85,
    diskCritical: 95,
    loadWarn: 80,
    loadCritical: 100,
    pressureWarn: 10,
    pressureCritical: 25,
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
    cpuCritical: numberValue(incoming.thresholds?.cpuCritical, next.thresholds.cpuCritical),
    memoryWarn: numberValue(incoming.thresholds?.memoryWarn, next.thresholds.memoryWarn),
    memoryCritical: numberValue(incoming.thresholds?.memoryCritical, next.thresholds.memoryCritical),
    diskWarn: numberValue(incoming.thresholds?.diskWarn, next.thresholds.diskWarn),
    diskCritical: numberValue(incoming.thresholds?.diskCritical, next.thresholds.diskCritical),
    loadWarn: numberValue(incoming.thresholds?.loadWarn, next.thresholds.loadWarn),
    loadCritical: numberValue(incoming.thresholds?.loadCritical, next.thresholds.loadCritical),
    pressureWarn: numberValue(incoming.thresholds?.pressureWarn, next.thresholds.pressureWarn),
    pressureCritical: numberValue(incoming.thresholds?.pressureCritical, next.thresholds.pressureCritical),
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
  validateRange("thresholds.cpuCritical", settings.thresholds.cpuCritical, 0, 100);
  validateRange("thresholds.memoryWarn", settings.thresholds.memoryWarn, 0, 100);
  validateRange("thresholds.memoryCritical", settings.thresholds.memoryCritical, 0, 100);
  validateRange("thresholds.diskWarn", settings.thresholds.diskWarn, 0, 100);
  validateRange("thresholds.diskCritical", settings.thresholds.diskCritical, 0, 100);
  validateRange("thresholds.loadWarn", settings.thresholds.loadWarn, 0, 100);
  validateRange("thresholds.loadCritical", settings.thresholds.loadCritical, 0, 100);
  validateRange("thresholds.pressureWarn", settings.thresholds.pressureWarn, 0, 100);
  validateRange("thresholds.pressureCritical", settings.thresholds.pressureCritical, 0, 100);
  validateThresholdPair("thresholds.cpu", settings.thresholds.cpuWarn, settings.thresholds.cpuCritical);
  validateThresholdPair("thresholds.memory", settings.thresholds.memoryWarn, settings.thresholds.memoryCritical);
  validateThresholdPair("thresholds.disk", settings.thresholds.diskWarn, settings.thresholds.diskCritical);
  validateThresholdPair("thresholds.load", settings.thresholds.loadWarn, settings.thresholds.loadCritical);
  validateThresholdPair("thresholds.pressure", settings.thresholds.pressureWarn, settings.thresholds.pressureCritical);
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

function validateThresholdPair(field: string, warn: number, critical: number): void {
  if (warn > critical) {
    throw new Error(`${field} warning threshold must be less than or equal to critical threshold`);
  }
}
