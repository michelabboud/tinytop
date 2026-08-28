const MIN_FREE_BYTES = 256 * 1024 * 1024;
const MAX_HISTORY_PAGE_SIZE = 10_000;

export const HISTORY_WINDOWS = Object.freeze({
  live: { label: "Live", durationMs: 5 * 60 * 1000, pageSize: 240, source: "raw" },
  "15m": { label: "15m", durationMs: 15 * 60 * 1000, pageSize: 900, source: "raw" },
  "1h": { label: "1h", durationMs: 60 * 60 * 1000, pageSize: 2_400, source: "raw" },
  "6h": { label: "6h", durationMs: 6 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "rollup" },
  "24h": { label: "24h", durationMs: 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "rollup" },
  "7d": { label: "7d", durationMs: 7 * 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "rollup" },
  "30d": { label: "30d", durationMs: 30 * 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "rollup" },
  "90d": { label: "90d", durationMs: 90 * 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "5m" },
  "1y": { label: "1y", durationMs: 365 * 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE, source: "1h" },
  all: { label: "All", durationMs: null, pageSize: MAX_HISTORY_PAGE_SIZE, source: "auto" },
});

const SOURCE_TIERS = Object.freeze({
  raw: { tier: "l1", setting: "retentionLadder.l1.keepDays" },
  rollup: { tier: "l2", setting: "retentionLadder.l2.keepDays" },
  "5m": { tier: "l3", setting: "retentionLadder.l3.enabled" },
  "1h": { tier: "l4", setting: "retentionLadder.l4.enabled" },
});

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

  if (windowKey === "all") {
    const sinceMs = retainedOldestMs(coverage);
    if (sinceMs !== null) return { ...config, sinceMs };

    const hasCoverageDetail = Array.isArray(coverage?.tiers) || coverage?.archive != null;
    return hasCoverageDetail
      ? { ...config, disabled: true, reason: "retentionLadder tiers/archive have no retained data" }
      : config;
  }

  const sourceTier = SOURCE_TIERS[config.source];
  if (!sourceTier || !Array.isArray(coverage?.tiers)) return config;
  if (
    config.source === "raw" &&
    Object.hasOwn(coverage, "snapshotJsonOldestMs") &&
    finiteTimestamp(coverage.snapshotJsonOldestMs) === null
  ) {
    return { ...config, disabled: true, reason: "retentionLadder.snapshotJsonKeepMinutes" };
  }
  const tier = coverage.tiers.find((candidate) => candidate?.tier === sourceTier.tier);
  if (!tier) return config;
  if (tier.enabled === false || Number(tier.bucketCount ?? 0) <= 0 || finiteTimestamp(tier.oldestMs) === null) {
    return { ...config, disabled: true, reason: sourceTier.setting };
  }
  return config;
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
    candidate.snapshotJsonKeepMinutes > previous.snapshotJsonKeepMinutes ||
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
    ["retentionLadder.snapshotJsonKeepMinutes", ladder?.snapshotJsonKeepMinutes, 60, 1_440],
    ["retentionLadder.detailIntervalSec", ladder?.detailIntervalSec, 15, 3_600],
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

  const pressureActive = Boolean(diskPressure?.active ?? diskPressure?.pressure);
  if (pressureActive && previous && growsFrom(ladder, previous)) {
    return [
      `disk pressure active: free ${diskPressure.freeBytes} < minFreeBytes ${diskPressure.minFreeBytes}; shrink first or free disk`,
    ];
  }
  return [];
}
