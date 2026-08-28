import { describe, expect, test } from "bun:test";

import {
  HISTORY_WINDOWS,
  historyWindowFor,
  validateRetentionLadder,
} from "../agent/assets/dashboard/ladder-rules.js";

const MIB = 1024 * 1024;

function ladder() {
  return {
    l1: { keepDays: 3 },
    l2: { keepDays: 30 },
    l3: { enabled: true, keepDays: 90 },
    l4: { enabled: true, keepDays: 730 },
    snapshotJsonKeepMinutes: 60,
    detailIntervalSec: 60,
    archive: {
      queryable: false,
      cold: false,
      coldAfterMonths: 12,
      directory: "",
    },
    diskCheck: {
      intervalMinutes: 60,
      minFreeBytes: 5 * 1024 * MIB,
    },
  };
}

describe("history ladder windows", () => {
  test("exposes every preset with its server history source", () => {
    expect(Object.keys(HISTORY_WINDOWS)).toEqual([
      "live",
      "15m",
      "1h",
      "6h",
      "24h",
      "7d",
      "30d",
      "90d",
      "1y",
      "all",
    ]);
    expect(Object.values(HISTORY_WINDOWS).map(({ source }) => source)).toEqual([
      "raw",
      "raw",
      "raw",
      "rollup",
      "rollup",
      "rollup",
      "rollup",
      "5m",
      "1h",
      "auto",
    ]);
  });

  test("disables the 90d preset when L3 is disabled", () => {
    const result = historyWindowFor("90d", {
      tiers: [{ tier: "l3", enabled: false, bucketCount: 10, oldestMs: 100, newestMs: 200 }],
    });

    expect(result).toMatchObject({ disabled: true, reason: "retentionLadder.l3.enabled" });
  });

  test("disables the 1y preset when L4 has no retained data", () => {
    const result = historyWindowFor("1y", {
      tiers: [{ tier: "l4", enabled: true, bucketCount: 0, oldestMs: null, newestMs: null }],
    });

    expect(result).toMatchObject({ disabled: true, reason: "retentionLadder.l4.enabled" });
  });

  test("keeps presets usable when an older runtime omits tier coverage", () => {
    expect(historyWindowFor("90d", { oldestCapturedAtMs: 100 }).disabled).toBe(false);
  });

  test("disables raw presets when coverage reports no retained snapshot JSON", () => {
    const result = historyWindowFor("1h", {
      snapshotJsonOldestMs: null,
      tiers: [{ tier: "l1", enabled: true, bucketCount: 10, oldestMs: 100, newestMs: 200 }],
    });

    expect(result).toMatchObject({
      disabled: true,
      reason: "retentionLadder.snapshotJsonKeepMinutes",
    });
  });

  test("starts all-history at the oldest tier or queryable archive bucket", () => {
    const result = historyWindowFor("all", {
      tiers: [
        { tier: "l1", enabled: true, bucketCount: 2, oldestMs: 500, newestMs: 700 },
        { tier: "l4", enabled: true, bucketCount: 2, oldestMs: 300, newestMs: 600 },
      ],
      archive: {
        queryable: { enabled: true, bucketCount: 4, oldestMs: 100, newestMs: 400 },
      },
    });

    expect(result).toMatchObject({ disabled: false, source: "auto", sinceMs: 100 });
  });
});

describe("retention ladder validation mirror", () => {
  test("accepts the server defaults", () => {
    expect(validateRetentionLadder(ladder(), null, null)).toEqual([]);
  });

  const invalidCases: Array<{
    name: string;
    mutate: (candidate: ReturnType<typeof ladder>) => void;
    message: string;
  }> = [
    {
      name: "L1 range",
      mutate: (candidate) => {
        candidate.l1.keepDays = 2;
      },
      message: "retentionLadder.l1.keepDays must be between 3 and 3650; observed 2",
    },
    {
      name: "L2 range",
      mutate: (candidate) => {
        candidate.l2.keepDays = 6;
      },
      message: "retentionLadder.l2.keepDays must be between 7 and 3650; observed 6",
    },
    {
      name: "L3 range",
      mutate: (candidate) => {
        candidate.l3.keepDays = 3651;
      },
      message: "retentionLadder.l3.keepDays must be between 0 and 3650; observed 3651",
    },
    {
      name: "L4 range",
      mutate: (candidate) => {
        candidate.l4.keepDays = 36501;
      },
      message: "retentionLadder.l4.keepDays must be between 0 and 36500; observed 36501",
    },
    {
      name: "snapshot JSON range",
      mutate: (candidate) => {
        candidate.snapshotJsonKeepMinutes = 59;
      },
      message: "retentionLadder.snapshotJsonKeepMinutes must be between 60 and 1440; observed 59",
    },
    {
      name: "detail interval range",
      mutate: (candidate) => {
        candidate.detailIntervalSec = 14;
      },
      message: "retentionLadder.detailIntervalSec must be between 15 and 3600; observed 14",
    },
    {
      name: "cold-after range",
      mutate: (candidate) => {
        candidate.archive.coldAfterMonths = 0;
      },
      message: "retentionLadder.archive.coldAfterMonths must be between 1 and 120; observed 0",
    },
    {
      name: "disk interval range",
      mutate: (candidate) => {
        candidate.diskCheck.intervalMinutes = 4;
      },
      message: "retentionLadder.diskCheck.intervalMinutes must be between 5 and 1440; observed 4",
    },
    {
      name: "minimum free bytes",
      mutate: (candidate) => {
        candidate.diskCheck.minFreeBytes = 256 * MIB - 1;
      },
      message: "retentionLadder.diskCheck.minFreeBytes must be at least 268435456; observed 268435455",
    },
    {
      name: "L3 monotonic retention",
      mutate: (candidate) => {
        candidate.l3.keepDays = 29;
      },
      message:
        "retentionLadder.l3.keepDays must be greater than or equal to retentionLadder.l2.keepDays (30) when retentionLadder.l3.enabled is true; observed 29",
    },
    {
      name: "L4 monotonic retention through L3",
      mutate: (candidate) => {
        candidate.l4.keepDays = 89;
      },
      message:
        "retentionLadder.l4.keepDays must be 0 (forever) or greater than or equal to retentionLadder.l3.keepDays (90) when retentionLadder.l4.enabled is true; observed 89",
    },
    {
      name: "L4 monotonic retention through L2",
      mutate: (candidate) => {
        candidate.l3.enabled = false;
        candidate.l4.keepDays = 29;
      },
      message:
        "retentionLadder.l4.keepDays must be 0 (forever) or greater than or equal to retentionLadder.l2.keepDays (30) when retentionLadder.l4.enabled is true; observed 29",
    },
    {
      name: "cold archive dependency",
      mutate: (candidate) => {
        candidate.archive.cold = true;
      },
      message:
        "retentionLadder.archive.cold requires retentionLadder.archive.queryable=true; observed cold=true, queryable=false",
    },
    {
      name: "archive directory",
      mutate: (candidate) => {
        candidate.archive.directory = "relative/archive";
      },
      message: 'retentionLadder.archive.directory must be empty or an absolute path; observed "relative/archive"',
    },
  ];

  for (const invalidCase of invalidCases) {
    test(`matches the server message for ${invalidCase.name}`, () => {
      const candidate = ladder();
      invalidCase.mutate(candidate);

      expect(validateRetentionLadder(candidate, null, null)).toEqual([invalidCase.message]);
    });
  }

  test("matches the disk-pressure growth refusal", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 31;

    expect(
      validateRetentionLadder(candidate, previous, {
        active: true,
        freeBytes: 100,
        minFreeBytes: 200,
      }),
    ).toEqual(["disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk"]);
  });

  test("allows a shrink while disk pressure is active", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 20;
    candidate.l3.keepDays = 90;

    expect(
      validateRetentionLadder(candidate, previous, {
        active: true,
        freeBytes: 100,
        minFreeBytes: 200,
      }),
    ).toEqual([]);
  });
});
