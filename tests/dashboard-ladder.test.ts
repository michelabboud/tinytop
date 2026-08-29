import { describe, expect, test } from "bun:test";

import {
  HISTORY_WINDOWS,
  describeDiskCoverage,
  describeImportPlan,
  exportFilenameFrom,
  fallbackWindowKey,
  historyWindowFor,
  isValidImportPlan,
  shouldFetchCoverage,
  validateRetentionLadder,
} from "../agent/assets/dashboard/ladder-rules.js";

const MIB = 1024 * 1024;

describe("history coverage polling", () => {
  test("skips a request while another coverage request is in flight", () => {
    expect(
      shouldFetchCoverage({ lastFetchedAtMs: 0, nowMs: 20_000, inFlight: true, force: true }),
    ).toBe(false);
  });

  test("allows a forced request when no coverage request is in flight", () => {
    expect(
      shouldFetchCoverage({ lastFetchedAtMs: 19_999, nowMs: 20_000, inFlight: false, force: true }),
    ).toBe(true);
  });

  test("allows a scheduled request after the minimum interval", () => {
    expect(
      shouldFetchCoverage({ lastFetchedAtMs: 5_000, nowMs: 20_000, inFlight: false, force: false }),
    ).toBe(true);
  });

  test("throttles a scheduled request within the minimum interval", () => {
    expect(
      shouldFetchCoverage({ lastFetchedAtMs: 5_001, nowMs: 20_000, inFlight: false, force: false }),
    ).toBe(false);
  });
});

describe("history disk coverage", () => {
  test("describes an unmeasured disk check without inventing zero free bytes", () => {
    expect(describeDiskCoverage({ freeBytes: null, minFreeBytes: 5 * 1024 * MIB, lastCheckMs: null })).toBe(
      "History disk check: not measured yet; minimum 5.0 GiB.",
    );
  });

  test("describes a measured disk check", () => {
    expect(describeDiskCoverage({ freeBytes: 12 * 1024 * MIB, minFreeBytes: 5 * 1024 * MIB })).toBe(
      "History disk check: 12 GiB free; minimum 5.0 GiB.",
    );
  });
});

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
  test("uses raw for short presets and auto with one 10000-point page from 6h up", () => {
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
      "auto",
      "auto",
      "auto",
      "auto",
      "auto",
      "auto",
      "auto",
    ]);
    expect(Object.values(HISTORY_WINDOWS).slice(3).map(({ pageSize }) => pageSize)).toEqual(
      Array(7).fill(10_000),
    );
  });

  test("disables 1y on the coarsest disabled tier that would have held it", () => {
    const result = historyWindowFor("1y", {
      tiers: [
        { tier: "l1", enabled: true, keepDays: 3 },
        { tier: "l2", enabled: true, keepDays: 30 },
        { tier: "l3", enabled: true, keepDays: 90 },
        { tier: "l4", enabled: false, keepDays: 730 },
      ],
    });

    expect(result).toMatchObject({ disabled: true, reason: "retentionLadder.l4.enabled" });
  });

  test("keeps 90d available through enabled L4 when L3 is disabled", () => {
    const result = historyWindowFor("90d", {
      tiers: [
        { tier: "l3", enabled: false, keepDays: 90 },
        { tier: "l4", enabled: true, keepDays: 730 },
      ],
    });

    expect(result).toMatchObject({ disabled: false });
  });

  test("disables 90d on L4 enabled when L3 is too short and L4 is disabled", () => {
    const result = historyWindowFor("90d", {
      tiers: [
        { tier: "l3", enabled: true, keepDays: 30 },
        { tier: "l4", enabled: false, keepDays: 730 },
      ],
    });

    expect(result).toMatchObject({ disabled: true, reason: "retentionLadder.l4.enabled" });
  });

  test("names the coarsest enabled keepDays when no enabled tier holds 30d", () => {
    const result = historyWindowFor("30d", {
      tiers: [
        { tier: "l1", enabled: true, keepDays: 3 },
        { tier: "l2", enabled: true, keepDays: 7 },
        { tier: "l3", enabled: true, keepDays: 14 },
        { tier: "l4", enabled: true, keepDays: 20 },
      ],
    });

    expect(result).toMatchObject({ disabled: true, reason: "retentionLadder.l4.keepDays" });
  });

  test("does not treat an absent tier record as holding the requested start", () => {
    const result = historyWindowFor("90d", {
      tiers: [{ tier: "l3", enabled: true, keepDays: 30 }],
    });

    expect(result).toMatchObject({ disabled: true, reason: "retentionLadder.l3.keepDays" });
  });

  test("keeps presets usable when an older runtime omits tier coverage", () => {
    expect(historyWindowFor("90d", { oldestCapturedAtMs: 100 }).disabled).toBe(false);
  });

  test("keeps raw presets usable before coverage has loaded", () => {
    expect(historyWindowFor("live", null)).toMatchObject({ disabled: false, source: "raw" });
  });

  test("keeps a long preset available when the queryable archive is enabled", () => {
    const result = historyWindowFor("1y", {
      tiers: [{ tier: "l4", enabled: false, keepDays: 30 }],
      archive: { queryable: { enabled: true } },
    });

    expect(result).toMatchObject({ disabled: false });
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
    expect(
      historyWindowFor("live", {
        snapshotJsonOldestMs: null,
        tiers: [],
      }),
    ).toMatchObject({ disabled: false });
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

  test("falls back from 1y to the nearest finer 90d preset", () => {
    const coverage = {
      tiers: [
        { tier: "l1", enabled: true, keepDays: 3 },
        { tier: "l2", enabled: true, keepDays: 30 },
        { tier: "l3", enabled: true, keepDays: 90 },
        { tier: "l4", enabled: false, keepDays: 730 },
      ],
    };

    expect(fallbackWindowKey("1y", coverage)).toBe("90d");
  });

  test("falls back from 1y to 30d when L3 retains only 30 days", () => {
    const coverage = {
      tiers: [
        { tier: "l1", enabled: true, keepDays: 3 },
        { tier: "l2", enabled: true, keepDays: 30 },
        { tier: "l3", enabled: true, keepDays: 30 },
        { tier: "l4", enabled: false, keepDays: 730 },
      ],
    };

    expect(fallbackWindowKey("1y", coverage)).toBe("30d");
  });

  test("falls back to 7d when every tier except seven-day L2 is disabled", () => {
    const coverage = {
      tiers: [
        { tier: "l1", enabled: false, keepDays: 3 },
        { tier: "l2", enabled: true, keepDays: 7 },
        { tier: "l3", enabled: false, keepDays: 90 },
        { tier: "l4", enabled: false, keepDays: 730 },
      ],
    };

    expect(fallbackWindowKey("1y", coverage)).toBe("7d");
  });

  test("keeps the requested key on older coverage without tiers", () => {
    expect(fallbackWindowKey("1y", { oldestCapturedAtMs: 100 })).toBe("1y");
  });

  test("disables Rust-only presets when coverage is unavailable", () => {
    expect(historyWindowFor("6h", { unavailable: true })).toMatchObject({
      disabled: true,
      reason: "runtime",
    });
    expect(historyWindowFor("1h", { unavailable: true })).toMatchObject({ disabled: false });
    expect(fallbackWindowKey("30d", { unavailable: true })).toBe("1h");
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

  test("refuses growth when the coverage pressure field is true", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 31;

    expect(
      validateRetentionLadder(candidate, previous, {
        active: false,
        pressure: true,
        freeBytes: 1000,
        minFreeBytes: 5000,
      }),
    ).toEqual(["disk pressure active: free 1000 < minFreeBytes 5000; shrink first or free disk"]);
  });

  test("allows growth when the coverage pressure field is false", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 31;

    expect(
      validateRetentionLadder(candidate, previous, {
        pressure: false,
        freeBytes: 1000,
        minFreeBytes: 5000,
      }),
    ).toEqual([]);
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

describe("settings transfer plan description", () => {
  test("can limit a save preview to retention consequences", () => {
    const plan = {
      wouldDelete: {},
      changedKeys: ["defaultTheme"],
      warnings: [],
    };

    expect(describeImportPlan(plan, ladder(), { retentionLadder: ladder() }, { includeOtherChanges: false })).toEqual(
      [],
    );
    expect(describeImportPlan(plan, ladder(), { retentionLadder: ladder() })).toEqual([
      "also changes: defaultTheme",
    ]);
  });

  test("describes deletions and queryable archive moves with server-computed counts", () => {
    const candidate = ladder();
    candidate.archive.queryable = true;

    expect(
      describeImportPlan(
        {
          wouldDelete: {
            l1Rows: 1_234,
            l2Buckets: 56,
            l3Buckets: 7,
            l4Buckets: 8,
            snapshotJsonRows: 90,
          },
          changedKeys: ["retentionLadder", "pollIntervalMs"],
          warnings: ["settings.bogus: unknown key ignored"],
        },
        candidate,
        { retentionLadder: ladder() },
      ),
    ).toEqual([
      "1,234 L1 rows",
      "56 L2 buckets",
      "7 L3 buckets",
      "8 L4 buckets (moved to the queryable archive)",
      "90 snapshot JSON blobs stripped",
      "also changes: pollIntervalMs",
      "settings.bogus: unknown key ignored",
    ]);

    candidate.archive.queryable = false;
    expect(
      describeImportPlan(
        { wouldDelete: { l4Buckets: 8 }, changedKeys: [], warnings: [] },
        candidate,
        { retentionLadder: ladder() },
      ),
    ).toEqual(["8 L4 buckets deleted"]);
  });

  test("describes disabled tiers as retained rather than deleted", () => {
    const candidate = ladder();
    candidate.l3.enabled = false;

    expect(
      describeImportPlan(
        { wouldDelete: {}, changedKeys: ["retentionLadder"], warnings: [] },
        candidate,
        { retentionLadder: ladder() },
      ),
    ).toEqual(["L3 disabled — its table is retained; reads fall through to the next tier"]);
  });

  test("describes archive disablement as keeping existing archive data", () => {
    const previous = ladder();
    previous.archive.queryable = true;
    previous.archive.cold = true;
    const candidate = ladder();

    expect(
      describeImportPlan(
        { wouldDelete: {}, changedKeys: ["retentionLadder"], warnings: [] },
        candidate,
        { retentionLadder: previous },
      ),
    ).toEqual([
      "queryable archive reads disabled — history-archive.sqlite is kept",
      "cold export stops — exported files are kept",
    ]);
  });

  test("uses an attachment filename when valid and falls back otherwise", () => {
    expect(exportFilenameFrom('attachment; filename="tinytop-settings-20240102-0304.json"', "fallback.json")).toBe(
      "tinytop-settings-20240102-0304.json",
    );
    expect(exportFilenameFrom("attachment", "fallback.json")).toBe("fallback.json");
  });

  test("rejects an empty dry-run plan", () => {
    expect(isValidImportPlan({})).toBe(false);
  });

  test("rejects a dry-run plan whose valid field is not true", () => {
    expect(
      isValidImportPlan({
        valid: null,
        wouldDelete: {
          l1Rows: 0,
          l2Buckets: 0,
          l3Buckets: 0,
          l4Buckets: 0,
          snapshotJsonRows: 0,
        },
      }),
    ).toBe(false);
  });

  test("rejects dry-run plans with missing or invalid deletion counts", () => {
    const validPlan = {
      valid: true,
      wouldDelete: {
        l1Rows: 0,
        l2Buckets: 0,
        l3Buckets: 0,
        l4Buckets: 0,
        snapshotJsonRows: 0,
      },
    };
    expect(isValidImportPlan({ valid: true, wouldDelete: {} })).toBe(false);
    expect(
      isValidImportPlan({
        ...validPlan,
        wouldDelete: { ...validPlan.wouldDelete, l4Buckets: -1 },
      }),
    ).toBe(false);
    expect(
      isValidImportPlan({
        ...validPlan,
        wouldDelete: { ...validPlan.wouldDelete, snapshotJsonRows: Number.NaN },
      }),
    ).toBe(false);
    expect(
      isValidImportPlan({
        ...validPlan,
        wouldDelete: { ...validPlan.wouldDelete, l2Buckets: 1.5 },
      }),
    ).toBe(false);
    expect(isValidImportPlan(validPlan)).toBe(true);
  });
});
