const MIN_FREE_BYTES = 256 * 1024 * 1024;
const MAX_HISTORY_PAGE_SIZE = 10_000;
const DAY_MS = 86_400_000;
const LONG_HISTORY_WINDOW_KEYS = new Set(["6h", "24h", "7d", "30d", "90d", "1y", "all"]);
const FALLBACK_WINDOW_KEYS = ["all", "1y", "90d", "30d", "7d", "24h", "6h", "1h", "15m", "live"];
const TIER_ORDER = new Map([
  ["l1", 1],
  ["l2", 2],
  ["l3", 3],
  ["l4", 4],
]);
const WOULD_DELETE_FIELDS = [
  "l1Rows",
  "l2Buckets",
  "l3Buckets",
  "l4Buckets",
  "processFastRows",
  "gpuSampleRows",
];

function formatCoverageBytes(bytes) {
  const numeric = Number(bytes);
  if (!Number.isFinite(numeric) || numeric <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = numeric;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function shouldFetchCoverage(
  { lastFetchedAtMs, nowMs, inFlight, force },
  minIntervalMs = 15_000,
) {
  if (inFlight) return false;
  if (force) return true;
  return Number(nowMs) - Number(lastFetchedAtMs) >= minIntervalMs;
}

export function describeDiskCoverage(disk) {
  const minimum = formatCoverageBytes(disk?.minFreeBytes);
  if (disk?.freeBytes == null) {
    return `History disk check: not measured yet; minimum ${minimum}.`;
  }
  if (disk?.pressure) {
    return `Disk pressure: ${formatCoverageBytes(disk.freeBytes)} free is below ${minimum}. Shrink history or free disk before extending retention.`;
  }
  return `History disk check: ${formatCoverageBytes(disk.freeBytes)} free; minimum ${minimum}.`;
}

export const HISTORY_WINDOWS = Object.freeze({
  live: { label: "Live", durationMs: 5 * 60 * 1000, pageSize: 240, source: "raw" },
  "15m": { label: "15m", durationMs: 15 * 60 * 1000, pageSize: 900, source: "raw" },
  "1h": { label: "1h", durationMs: 60 * 60 * 1000, pageSize: 2_400, source: "raw" },
  "6h": { label: "6h", durationMs: 6 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
  "24h": { label: "24h", durationMs: 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
  "7d": { label: "7d", durationMs: 7 * DAY_MS, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
  "30d": { label: "30d", durationMs: 30 * DAY_MS, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
  "90d": { label: "90d", durationMs: 90 * DAY_MS, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
  "1y": { label: "1y", durationMs: 365 * DAY_MS, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
  all: { label: "All", durationMs: null, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
});

export function normalizeHistorySamples(samples, source) {
  const byTimestamp = new Map();
  if (!Array.isArray(samples)) return [];

  for (const sample of samples) {
    if (!sample?.snapshot) continue;
    const snapshotTimestamp = Date.parse(sample.snapshot.timestamp);
    const capturedAt = Number.isFinite(Number(sample.capturedAtMs))
      ? Number(sample.capturedAtMs)
      : Number.isFinite(snapshotTimestamp)
        ? snapshotTimestamp
        : Date.now();
    byTimestamp.set(capturedAt, {
      capturedAt,
      snapshot: sample.snapshot,
      source: typeof sample.source === "string" ? sample.source : source,
    });
  }

  return Array.from(byTimestamp.values()).sort((left, right) => left.capturedAt - right.capturedAt);
}

export function formatPressureValue(value) {
  return Number.isFinite(value) ? value.toFixed(2) : "—";
}

export function pressureMaximum(values) {
  const finiteValues = values.filter((value) => Number.isFinite(value));
  return finiteValues.length > 0 ? Math.max(...finiteValues) : null;
}

export function formatCount(value) {
  return Number.isFinite(value) ? String(value) : "—";
}

export function formatGpuPercent(value) {
  return Number.isFinite(value) ? `${value.toFixed(1)}%` : "—";
}

export function formatGpuMemory(usedBytes, totalBytes) {
  if (!Number.isFinite(usedBytes)) return "—";
  if (!Number.isFinite(totalBytes)) return formatCoverageBytes(usedBytes);
  return `${formatCoverageBytes(usedBytes)} / ${formatCoverageBytes(totalBytes)}`;
}

export function formatGpuTemperature(value) {
  return Number.isFinite(value) ? `${Math.round(value)} °C` : "";
}

export function describeGpuAdapter(adapter) {
  const name = typeof adapter?.name === "string" && adapter.name.length > 0 ? adapter.name : adapter?.id;
  return {
    name,
    meta: `${adapter?.vendor} · ${adapter?.driver}`,
    busy: formatGpuPercent(adapter?.busyPercent),
    memory: formatGpuMemory(adapter?.memoryUsedBytes, adapter?.memoryTotalBytes),
    temperature: formatGpuTemperature(adapter?.temperatureC),
  };
}

export function gpuColumnVisible(processes) {
  return Array.isArray(processes) && processes.some((process) => Number.isFinite(process?.gpuPercent));
}

export function gpuPercentSortValue(process) {
  return Number.isFinite(process?.gpuPercent) ? process.gpuPercent : -1;
}

function finiteTimestamp(value) {
  const timestamp = Number(value);
  return Number.isFinite(timestamp) && timestamp > 0 ? timestamp : null;
}

function retainedOldestMs(coverage) {
  const timestamps = [];
  if (Array.isArray(coverage?.tiers)) {
    for (const tier of coverage.tiers) {
      if (tier?.enabled === false || Number(tier?.bucketCount ?? 0) <= 0) continue;
      const oldestMs = finiteTimestamp(tier?.oldestMs);
      if (oldestMs !== null) timestamps.push(oldestMs);
    }
  }

  const queryable = coverage?.archive?.queryable;
  if (queryable?.enabled !== false && Number(queryable?.bucketCount ?? 0) > 0) {
    const oldestMs = finiteTimestamp(queryable?.oldestMs);
    if (oldestMs !== null) timestamps.push(oldestMs);
  }

  const legacyOldestMs = finiteTimestamp(coverage?.oldestCapturedAtMs);
  if (legacyOldestMs !== null) timestamps.push(legacyOldestMs);
  return timestamps.length > 0 ? Math.min(...timestamps) : null;
}

export function historyWindowFor(key, coverage) {
  const windowKey = Object.hasOwn(HISTORY_WINDOWS, key) ? key : "live";
  const config = { ...HISTORY_WINDOWS[windowKey], disabled: false, reason: "" };

  if (coverage?.unavailable === true && LONG_HISTORY_WINDOW_KEYS.has(windowKey)) {
    return { ...config, disabled: true, reason: "runtime" };
  }

  if (windowKey === "all") {
    const sinceMs = retainedOldestMs(coverage);
    if (sinceMs !== null) return { ...config, sinceMs };

    const hasCoverageDetail = Array.isArray(coverage?.tiers) || coverage?.archive != null;
    return hasCoverageDetail
      ? { ...config, disabled: true, reason: "retentionLadder tiers/archive have no retained data" }
      : config;
  }

  if (windowKey === "live") return config;

  if (config.source === "raw" || !Array.isArray(coverage?.tiers)) return config;
  if (coverage?.archive?.queryable?.enabled === true) return config;

  const durationMs = Number(config.durationMs);
  const holdsStart = (tier) => {
    const keepDays = Number(tier?.keepDays);
    return Number.isFinite(keepDays) && (keepDays === 0 || keepDays * DAY_MS >= durationMs);
  };
  if (coverage.tiers.some((tier) => tier?.enabled === true && holdsStart(tier))) return config;

  const byCoarsest = (left, right) => (TIER_ORDER.get(right?.tier) ?? 0) - (TIER_ORDER.get(left?.tier) ?? 0);
  const disabledTier = coverage.tiers
    .filter((tier) => tier?.enabled === false && holdsStart(tier))
    .sort(byCoarsest)[0];
  if (disabledTier) {
    return { ...config, disabled: true, reason: `retentionLadder.${disabledTier.tier}.enabled` };
  }

  const coarsestEnabled = coverage.tiers.filter((tier) => tier?.enabled === true).sort(byCoarsest)[0];
  if (coarsestEnabled) {
    return { ...config, disabled: true, reason: `retentionLadder.${coarsestEnabled.tier}.keepDays` };
  }

  const coarsestPresent = coverage.tiers.filter((tier) => TIER_ORDER.has(tier?.tier)).sort(byCoarsest)[0];
  if (coarsestPresent) {
    return { ...config, disabled: true, reason: `retentionLadder.${coarsestPresent.tier}.enabled` };
  }
  return { ...config, disabled: true, reason: "retentionLadder tiers" };
}

export function fallbackWindowKey(key, coverage) {
  const windowKey = Object.hasOwn(HISTORY_WINDOWS, key) ? key : "live";
  if (!historyWindowFor(windowKey, coverage).disabled) return windowKey;

  const start = FALLBACK_WINDOW_KEYS.indexOf(windowKey);
  for (const candidate of FALLBACK_WINDOW_KEYS.slice(start + 1)) {
    if (candidate === "live" || !historyWindowFor(candidate, coverage).disabled) return candidate;
  }
  return "live";
}

export function ladderCapabilityFrom(settingsOrNull) {
  return Boolean(
    settingsOrNull &&
      typeof settingsOrNull === "object" &&
      Object.hasOwn(settingsOrNull, "retentionLadder"),
  );
}

export function otelCapabilityFrom(settingsOrNull) {
  return Boolean(
    settingsOrNull &&
      typeof settingsOrNull === "object" &&
      settingsOrNull.otel &&
      typeof settingsOrNull.otel === "object" &&
      !Array.isArray(settingsOrNull.otel),
  );
}

export function settingsPutPayload(settings, retentionLadderAvailable, otelAvailable = retentionLadderAvailable) {
  const payload = { ...settings };
  if (!retentionLadderAvailable) delete payload.retentionLadder;
  if (!otelAvailable) delete payload.otel;
  return payload;
}

const OTEL_PROTOCOL_ERROR = "otel.protocol must be one of http/protobuf";
const OTEL_ENDPOINT_ERROR = "otel.endpoint must be an http:// or https:// URL with a host and without credentials";
const OTEL_INTERVAL_ERROR = "otel.intervalSec must be between 5 and 3600";
const OTEL_HEADERS_ERROR = "otel.headersEnvVar must match ^[A-Z][A-Z0-9_]*$";
export const OTEL_HEADERS_RESERVED_ERROR =
  "otel.headersEnvVar must not be OTEL_EXPORTER_OTLP_HEADERS or OTEL_EXPORTER_OTLP_METRICS_HEADERS; tinytop reads headers only from its own variable";
const OTEL_SERVICE_ERROR = "otel.serviceName must be 1–128 characters without control characters";
const OTEL_ATTRIBUTES_ERROR =
  "otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters";
const OTEL_SECRET_SHAPED_KEY_ERROR =
  "otel.resourceAttributes keys must not be secret-shaped (no segment may be secret, token, password, passwd, apikey, api_key, authorization, bearer or credential)";
export const SECRET_SHAPED_KEY_WORDS = [
  "secret",
  "token",
  "password",
  "passwd",
  "apikey",
  "api_key",
  "authorization",
  "bearer",
  "credential",
];

function hasControlCharacters(value) {
  return /\p{Cc}/u.test(value);
}

function validResourceAttributeKey(key) {
  const characters = Array.from(key);
  return characters.length <= 64 && /^[a-z][a-z0-9._]*$/.test(key);
}

function secretShapedResourceAttributeKey(key) {
  return key.split(".").some((segment) =>
    SECRET_SHAPED_KEY_WORDS.includes(segment) ||
    segment.split("_").some((part) => SECRET_SHAPED_KEY_WORDS.includes(part)),
  );
}

function validResourceAttributes(attributes) {
  if (!attributes || typeof attributes !== "object" || Array.isArray(attributes)) return false;
  const entries = Object.entries(attributes);
  if (entries.length > 32) return false;
  return entries.every(([key, value]) => {
    const valueCharacters = typeof value === "string" ? Array.from(value) : [];
    return (
      validResourceAttributeKey(key) &&
      typeof value === "string" &&
      valueCharacters.length <= 256 &&
      !hasControlCharacters(value)
    );
  });
}

function validEndpointAuthority(authority) {
  if (authority.length === 0 || authority.includes("@")) return false;
  if (authority.startsWith("[")) {
    const closeBracket = authority.indexOf("]");
    return closeBracket > 1;
  }
  const portSeparator = authority.lastIndexOf(":");
  const host = portSeparator >= 0 ? authority.slice(0, portSeparator) : authority;
  return host.length > 0;
}

export function validateOtelSettings(otel) {
  const settings = otel && typeof otel === "object" && !Array.isArray(otel) ? otel : {};
  if (settings.protocol !== "http/protobuf") return [OTEL_PROTOCOL_ERROR];

  const endpoint = typeof settings.endpoint === "string" ? settings.endpoint : "";
  const endpointMatch = /^(?:http|https):\/\/([^/?#]*)/u.exec(endpoint);
  if (
    !endpointMatch ||
    !validEndpointAuthority(endpointMatch[1]) ||
    /\s|\p{Cc}/u.test(endpoint)
  ) {
    return [OTEL_ENDPOINT_ERROR];
  }

  if (!Number.isInteger(settings.intervalSec) || settings.intervalSec < 5 || settings.intervalSec > 3600) {
    return [OTEL_INTERVAL_ERROR];
  }
  if (typeof settings.headersEnvVar !== "string" || !/^[A-Z][A-Z0-9_]*$/u.test(settings.headersEnvVar)) {
    return [OTEL_HEADERS_ERROR];
  }
  if (
    settings.headersEnvVar === "OTEL_EXPORTER_OTLP_HEADERS" ||
    settings.headersEnvVar === "OTEL_EXPORTER_OTLP_METRICS_HEADERS"
  ) {
    return [OTEL_HEADERS_RESERVED_ERROR];
  }
  if (
    typeof settings.serviceName !== "string" ||
    Array.from(settings.serviceName).length < 1 ||
    Array.from(settings.serviceName).length > 128 ||
    hasControlCharacters(settings.serviceName)
  ) {
    return [OTEL_SERVICE_ERROR];
  }
  if (!validResourceAttributes(settings.resourceAttributes)) return [OTEL_ATTRIBUTES_ERROR];
  if (Object.keys(settings.resourceAttributes).some(secretShapedResourceAttributeKey)) {
    return [OTEL_SECRET_SHAPED_KEY_ERROR];
  }
  return [];
}

export function parseResourceAttributes(text) {
  const attributes = {};
  const errors = [];
  const lines = String(text ?? "").split(/\r?\n/u);
  for (const [index, rawLine] of lines.entries()) {
    if (rawLine.trim().length === 0) continue;
    const separator = rawLine.indexOf("=");
    const lineNumber = index + 1;
    if (separator < 1) {
      errors.push(`line ${lineNumber}: expected key=value`);
      continue;
    }
    const key = rawLine.slice(0, separator).trim();
    const value = rawLine.slice(separator + 1);
    if (!validResourceAttributeKey(key) || typeof value !== "string" || Array.from(value).length > 256 || hasControlCharacters(value)) {
      errors.push(`line ${lineNumber}: ${OTEL_ATTRIBUTES_ERROR}`);
      continue;
    }
    if (secretShapedResourceAttributeKey(key)) {
      errors.push(`line ${lineNumber}: ${OTEL_SECRET_SHAPED_KEY_ERROR}`);
      continue;
    }
    if (!Object.hasOwn(attributes, key) && Object.keys(attributes).length >= 32) {
      errors.push(`line ${lineNumber}: ${OTEL_ATTRIBUTES_ERROR}`);
      continue;
    }
    attributes[key] = value;
  }
  return { attributes, errors };
}

export function formatResourceAttributes(attributes) {
  if (!attributes || typeof attributes !== "object" || Array.isArray(attributes)) return "";
  return Object.entries(attributes)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
}

function formatOtelTime(timestampMs) {
  const timestamp = Number(timestampMs);
  if (!Number.isFinite(timestamp) || timestamp <= 0) return "-";
  return new Date(timestamp).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

export function describeFilesystemFreshness(snapshot, pollMs) {
  const capturedAtMs = snapshot?.filesystemsCapturedAtMs;
  const snapshotAtMs = Date.parse(snapshot?.timestamp);
  if (!Number.isFinite(capturedAtMs) || !Number.isFinite(snapshotAtMs)) return null;
  if (snapshotAtMs - capturedAtMs <= pollMs) return null;
  return `as of ${formatOtelTime(capturedAtMs)}`;
}

export function describeOtelCoverage(otel) {
  if (!otel || typeof otel !== "object" || otel.enabled !== true) return "OTel — off";
  const endpoint = typeof otel.endpoint === "string" ? otel.endpoint : "-";
  const interval = Number.isFinite(Number(otel.intervalSec)) ? Number(otel.intervalSec) : "-";
  const lastSuccess = Number(otel.lastSuccessMs);
  const lastFailure = Number(otel.lastFailureMs);
  let description = `OTel → ${endpoint} every ${interval} s · last success ${formatOtelTime(lastSuccess)} · failures ${Number(otel.failures ?? 0)}`;
  if (Number.isFinite(lastFailure) && (!Number.isFinite(lastSuccess) || lastFailure > lastSuccess) && otel.lastError) {
    description += ` · last error: ${otel.lastError}`;
  }
  return description;
}

export function exportFilenameFrom(headerValue, fallback) {
  const match = typeof headerValue === "string" ? /(?:^|;)\s*filename="([^"]+)"/i.exec(headerValue) : null;
  return match?.[1] || fallback;
}

export function isValidImportPlan(plan) {
  if (!plan || typeof plan !== "object" || plan.valid !== true) return false;
  const wouldDelete = plan.wouldDelete;
  if (!wouldDelete || typeof wouldDelete !== "object" || Array.isArray(wouldDelete)) return false;
  return WOULD_DELETE_FIELDS.every((field) => {
    const value = field === "gpuSampleRows" && !Object.hasOwn(wouldDelete, field) ? 0 : wouldDelete[field];
    return typeof value === "number" && Number.isFinite(value) && Number.isInteger(value) && value >= 0;
  });
}

function nonZeroCount(value) {
  const count = Number(value);
  return Number.isFinite(count) && count !== 0 ? count : null;
}

export function describeImportPlan(
  plan,
  candidateLadder,
  previousSettings,
  { includeOtherChanges = true } = {},
) {
  const lines = [];
  const wouldDelete = plan?.wouldDelete ?? {};
  const addCount = (field, label, suffix = "") => {
    const count = nonZeroCount(wouldDelete[field]);
    if (count !== null) lines.push(`${count.toLocaleString()} ${label}${suffix}`);
  };

  const ladderLinesStart = lines.length;
  addCount("l1Rows", "L1 rows");
  addCount("l2Buckets", "L2 buckets");
  addCount("l3Buckets", "L3 buckets");
  addCount(
    "l4Buckets",
    "L4 buckets",
    candidateLadder?.archive?.queryable ? " (moved to the queryable archive)" : " deleted",
  );
  addCount("processFastRows", "fast process rows", " deleted");
  addCount("gpuSampleRows", "GPU rows", " deleted");

  const previousLadder = previousSettings?.retentionLadder;
  for (const tier of ["l3", "l4"]) {
    if (previousLadder?.[tier]?.enabled && !candidateLadder?.[tier]?.enabled) {
      lines.push(`${tier.toUpperCase()} disabled — its table is retained; reads fall through to the next tier`);
    }
  }
  if (previousLadder?.archive?.queryable && !candidateLadder?.archive?.queryable) {
    lines.push("queryable archive reads disabled — history-archive.sqlite is kept");
  }
  if (previousLadder?.archive?.cold && !candidateLadder?.archive?.cold) {
    lines.push("cold export stops — exported files are kept");
  }
  const ladderDescribed = lines.length > ladderLinesStart;

  if (includeOtherChanges) {
    if (Array.isArray(plan?.changedKeys) && plan.changedKeys.includes("retentionLadder") && !ladderDescribed) {
      lines.push("retention ladder changes — no stored history is affected");
    }
    const otherChangedKeys = Array.isArray(plan?.changedKeys)
      ? plan.changedKeys.filter((key) => key !== "retentionLadder")
      : [];
    if (otherChangedKeys.length > 0) lines.push(`also changes: ${otherChangedKeys.join(", ")}`);
    if (Array.isArray(plan?.warnings)) lines.push(...plan.warnings);
    if (Array.isArray(plan?.changedKeys) && plan.changedKeys.length === 0 && lines.length === 0) {
      lines.push("no settings change — the document matches the current settings");
    }
  }
  return lines;
}

function rangeError(field, value, min, max) {
  return Number.isInteger(value) && value >= min && value <= max
    ? null
    : `${field} must be between ${min} and ${max}; observed ${value}`;
}

function isAbsolutePath(path) {
  return path === "" || path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\");
}

function enabledHorizonGrew(candidate, previous, zeroIsForever = false) {
  if (!candidate.enabled) return false;
  if (!previous.enabled) return true;
  if (zeroIsForever) {
    return (
      (candidate.keepDays === 0 && previous.keepDays !== 0) ||
      (candidate.keepDays !== 0 && previous.keepDays !== 0 && candidate.keepDays > previous.keepDays)
    );
  }
  return candidate.keepDays > previous.keepDays;
}

function growsFrom(candidate, previous) {
  return (
    candidate.l1.keepDays > previous.l1.keepDays ||
    candidate.l2.keepDays > previous.l2.keepDays ||
    enabledHorizonGrew(candidate.l3, previous.l3) ||
    enabledHorizonGrew(candidate.l4, previous.l4, true) ||
    candidate.processFastKeepHours > previous.processFastKeepHours ||
    (candidate.archive.queryable && !previous.archive.queryable) ||
    (candidate.archive.cold && !previous.archive.cold)
  );
}

export function validateRetentionLadder(ladder, previous, diskPressure) {
  const checks = [
    ["retentionLadder.l1.keepDays", ladder?.l1?.keepDays, 3, 3_650],
    ["retentionLadder.l2.keepDays", ladder?.l2?.keepDays, 7, 3_650],
    ["retentionLadder.l3.keepDays", ladder?.l3?.keepDays, 0, 3_650],
    ["retentionLadder.l4.keepDays", ladder?.l4?.keepDays, 0, 36_500],
    ["retentionLadder.detailIntervalSec", ladder?.detailIntervalSec, 15, 3_600],
    ["retentionLadder.processFastKeepHours", ladder?.processFastKeepHours, 1, 72],
    ["retentionLadder.archive.coldAfterMonths", ladder?.archive?.coldAfterMonths, 1, 120],
    ["retentionLadder.diskCheck.intervalMinutes", ladder?.diskCheck?.intervalMinutes, 5, 1_440],
  ];
  for (const [field, value, min, max] of checks) {
    const error = rangeError(field, value, min, max);
    if (error) return [error];
  }

  if (ladder.diskCheck.minFreeBytes < MIN_FREE_BYTES) {
    return [
      `retentionLadder.diskCheck.minFreeBytes must be at least ${MIN_FREE_BYTES}; observed ${ladder.diskCheck.minFreeBytes}`,
    ];
  }
  if (ladder.l3.enabled && ladder.l3.keepDays < ladder.l2.keepDays) {
    return [
      `retentionLadder.l3.keepDays must be greater than or equal to retentionLadder.l2.keepDays (${ladder.l2.keepDays}) when retentionLadder.l3.enabled is true; observed ${ladder.l3.keepDays}`,
    ];
  }
  if (ladder.l4.enabled && ladder.l4.keepDays !== 0) {
    const requiredField = ladder.l3.enabled ? "retentionLadder.l3.keepDays" : "retentionLadder.l2.keepDays";
    const requiredDays = ladder.l3.enabled ? ladder.l3.keepDays : ladder.l2.keepDays;
    if (ladder.l4.keepDays < requiredDays) {
      return [
        `retentionLadder.l4.keepDays must be 0 (forever) or greater than or equal to ${requiredField} (${requiredDays}) when retentionLadder.l4.enabled is true; observed ${ladder.l4.keepDays}`,
      ];
    }
  }
  if (ladder.archive.cold && !ladder.archive.queryable) {
    return [
      "retentionLadder.archive.cold requires retentionLadder.archive.queryable=true; observed cold=true, queryable=false",
    ];
  }
  if (!isAbsolutePath(ladder.archive.directory)) {
    return [
      `retentionLadder.archive.directory must be empty or an absolute path; observed ${JSON.stringify(ladder.archive.directory)}`,
    ];
  }

  const pressureActive = Boolean(diskPressure?.pressure);
  if (pressureActive && previous && growsFrom(ladder, previous)) {
    return [
      `disk pressure active: free ${diskPressure.freeBytes} < minFreeBytes ${diskPressure.minFreeBytes}; shrink first or free disk`,
    ];
  }
  return [];
}
