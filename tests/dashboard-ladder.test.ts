import { describe, expect, test } from "bun:test";
import * as ladderRules from "../agent/assets/dashboard/ladder-rules.js";

import {
  HISTORY_WINDOWS,
  describeDiskCoverage,
  describeFilesystemFreshness,
  describeImportPlan,
  exportFilenameFrom,
  fallbackWindowKey,
  historyWindowFor,
  isValidImportPlan,
  describeOtelCoverage,
  formatResourceAttributes,
  otelCapabilityFrom,
  parseResourceAttributes,
  shouldFetchCoverage,
  settingsPutPayload,
  validateOtelSettings,
  validateRetentionLadder,
} from "../agent/assets/dashboard/ladder-rules.js";

const MIB = 1024 * 1024;

describe("filesystem freshness", () => {
  const snapshotMs = Date.parse("2026-08-29T12:00:00.000Z");
  const pollMs = 1_500;

  test("is hidden when the filesystem capture time is absent", () => {
    expect(describeFilesystemFreshness({ timestamp: "2026-08-29T12:00:00.000Z" }, pollMs)).toBeNull();
  });

  test("is hidden when filesystems were captured with the snapshot", () => {
    expect(
      describeFilesystemFreshness(
        { timestamp: "2026-08-29T12:00:00.000Z", filesystemsCapturedAtMs: snapshotMs },
        pollMs,
      ),
    ).toBeNull();
  });

  test("is hidden at exactly one poll old", () => {
    expect(
      describeFilesystemFreshness(
        { timestamp: "2026-08-29T12:00:00.000Z", filesystemsCapturedAtMs: snapshotMs - pollMs },
        pollMs,
      ),
    ).toBeNull();
  });

  test("shows the capture time when older than one poll", () => {
    const capturedAtMs = snapshotMs - pollMs - 1;
    const formatted = new Date(capturedAtMs).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
    expect(
      describeFilesystemFreshness(
        { timestamp: "2026-08-29T12:00:00.000Z", filesystemsCapturedAtMs: capturedAtMs },
        pollMs,
      ),
    ).toBe(`as of ${formatted}`);
  });
});

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
    processFastKeepHours: 24,
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
      name: "fast process history range",
      mutate: (candidate) => {
        candidate.processFastKeepHours = 0;
      },
      message: "retentionLadder.processFastKeepHours must be between 1 and 72; observed 0",
    },
    {
      name: "fast process history upper range",
      mutate: (candidate) => {
        candidate.processFastKeepHours = 73;
      },
      message: "retentionLadder.processFastKeepHours must be between 1 and 72; observed 73",
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

  test("accepts the fast process history boundaries and default", () => {
    for (const processFastKeepHours of [1, 24, 72]) {
      const candidate = ladder();
      candidate.processFastKeepHours = processFastKeepHours;
      expect(validateRetentionLadder(candidate, null, null)).toEqual([]);
    }
  });

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

  test("refuses fast process history growth while disk pressure is active", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.processFastKeepHours = previous.processFastKeepHours + 1;

    expect(
      validateRetentionLadder(candidate, previous, {
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

describe("OpenTelemetry dashboard rules", () => {
  const endpointError =
    "otel.endpoint must be an http:// or https:// URL with a host and without credentials";
  const attributesError =
    "otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters";
  const secretShapedKeyError =
    "otel.resourceAttributes keys must not be secret-shaped (no segment may be secret, token, password, passwd, apikey, api_key, authorization, bearer or credential)";
  const reservedHeadersError =
    "otel.headersEnvVar must not be OTEL_EXPORTER_OTLP_HEADERS or OTEL_EXPORTER_OTLP_METRICS_HEADERS; tinytop reads headers only from its own variable";

  test("exports the shared secret-shaped key words and reserved-header message", () => {
    expect(ladderRules.SECRET_SHAPED_KEY_WORDS).toEqual([
      "secret",
      "token",
      "password",
      "passwd",
      "apikey",
      "api_key",
      "authorization",
      "bearer",
      "credential",
    ]);
    expect(ladderRules.OTEL_HEADERS_RESERVED_ERROR).toBe(reservedHeadersError);
  });

  test("detects OTel capability only when settings has an otel object", () => {
    expect(otelCapabilityFrom(null)).toBe(false);
    expect(otelCapabilityFrom({})).toBe(false);
    expect(otelCapabilityFrom({ otel: null })).toBe(false);
    expect(otelCapabilityFrom({ otel: { enabled: false } })).toBe(true);
  });

  test("parses resource attributes while ignoring blank lines", () => {
    expect(parseResourceAttributes("\nservice.version=1\n\ndeployment.environment=test\n")).toEqual({
      attributes: { "deployment.environment": "test", "service.version": "1" },
      errors: [],
    });
  });

  test("reports malformed resource attributes with their line number", () => {
    const result = parseResourceAttributes("good=value\nbad line\n");
    expect(result.attributes).toEqual({ good: "value" });
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0]).toContain("line 2");
  });

  test("reports a secret-shaped resource attribute key with its line number", () => {
    expect(parseResourceAttributes("auth.token=value")).toEqual({
      attributes: {},
      errors: ["line 1: " + secretShapedKeyError],
    });
  });

  test("preserves valid leading and trailing spaces in resource attribute values", () => {
    // Break caught: the settings UI silently changes an otherwise valid server value.
    expect(parseResourceAttributes("deployment.note=  keep these spaces  ")).toEqual({
      attributes: { "deployment.note": "  keep these spaces  " },
      errors: [],
    });
  });

  test("refuses secret-shaped resource attribute key segments while accepting ordinary keys", () => {
    for (const key of ["auth.token", "api_key", "service.api_key", "my_token"]) {
      const result = validateOtelSettings({
        enabled: false,
        endpoint: "http://127.0.0.1:4318/v1/metrics",
        protocol: "http/protobuf",
        intervalSec: 60,
        headersEnvVar: "TINYTOP_OTEL_HEADERS",
        serviceName: "tinytop",
        resourceAttributes: { [key]: "value" },
      });
      expect(result).toEqual([secretShapedKeyError]);
    }
    expect(
      validateOtelSettings({
        enabled: false,
        endpoint: "http://127.0.0.1:4318/v1/metrics",
        protocol: "http/protobuf",
        intervalSec: 60,
        headersEnvVar: "TINYTOP_OTEL_HEADERS",
        serviceName: "tinytop",
        resourceAttributes: { "deployment.environment": "production" },
      }),
    ).toEqual([]);
  });

  test("rejects C1 control characters in resource attribute values", () => {
    // Break caught: the browser accepts a value that Rust char::is_control refuses.
    const result = parseResourceAttributes("deployment.note=before\u0085after");
    expect(result.attributes).toEqual({});
    expect(result.errors).toEqual([
      "line 1: otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters",
    ]);
  });

  test("names the line that introduces a thirty-third resource attribute", () => {
    // Break caught: the textarea reports the block limit without identifying the offending line.
    const text = Array.from({ length: 33 }, (_, index) => `key.${index}=value`).join("\n");
    const result = parseResourceAttributes(text);
    expect(Object.keys(result.attributes)).toHaveLength(32);
    expect(result.errors).toEqual([
      "line 33: otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters",
    ]);
  });

  test("formats resource attributes in sorted-key order and round-trips", () => {
    const attributes = { "z.last": "2", "a.first": "1", "m.middle": "x=y" };
    const formatted = formatResourceAttributes(attributes);
    expect(formatted).toBe("a.first=1\nm.middle=x=y\nz.last=2");
    expect(parseResourceAttributes(formatted)).toEqual({ attributes, errors: [] });
  });

  test("mirrors each OTel validation rule and its server message", () => {
    const base = {
      enabled: false,
      endpoint: "http://127.0.0.1:4318/v1/metrics",
      protocol: "http/protobuf",
      intervalSec: 60,
      headersEnvVar: "TINYTOP_OTEL_HEADERS",
      serviceName: "tinytop",
      resourceAttributes: {},
    };
    const cases = [
      [{ ...base, protocol: "grpc" }, "otel.protocol must be one of http/protobuf"],
      [{ ...base, endpoint: "collector:4318" }, endpointError],
      [{ ...base, endpoint: "http:///v1/metrics" }, endpointError],
      [{ ...base, endpoint: "https://bad host/v1/metrics" }, endpointError],
      [{ ...base, endpoint: "https://collector.example/v1 /metrics" }, endpointError],
      [{ ...base, endpoint: "http://:4318/v1/metrics" }, endpointError],
      [{ ...base, endpoint: "https://user:sekrit@collector/v1/metrics" }, endpointError],
      [{ ...base, endpoint: "https://@collector/v1/metrics" }, endpointError],
      [{ ...base, intervalSec: 4 }, "otel.intervalSec must be between 5 and 3600"],
      [{ ...base, intervalSec: 3601 }, "otel.intervalSec must be between 5 and 3600"],
      [{ ...base, headersEnvVar: "tinytop_headers" }, "otel.headersEnvVar must match ^[A-Z][A-Z0-9_]*$"],
      [{ ...base, headersEnvVar: "1TINYTOP_HEADERS" }, "otel.headersEnvVar must match ^[A-Z][A-Z0-9_]*$"],
      [{ ...base, headersEnvVar: "OTEL_EXPORTER_OTLP_HEADERS" }, reservedHeadersError],
      [{ ...base, headersEnvVar: "OTEL_EXPORTER_OTLP_METRICS_HEADERS" }, reservedHeadersError],
      [{ ...base, serviceName: "" }, "otel.serviceName must be 1–128 characters without control characters"],
      [{ ...base, serviceName: "x".repeat(129) }, "otel.serviceName must be 1–128 characters without control characters"],
      [{ ...base, resourceAttributes: Object.fromEntries(Array.from({ length: 33 }, (_, i) => [`key.${i}`, "v"])) }, attributesError],
      [{ ...base, resourceAttributes: { "Bad-Key": "v" } }, attributesError],
      [{ ...base, resourceAttributes: { ["a".repeat(65)]: "v" } }, attributesError],
    ];
    for (const [candidate, message] of cases) expect(validateOtelSettings(candidate)).toEqual([message]);
    for (const endpoint of [
      "http://[::1]:4318/v1/metrics",
      "https://collector.example/v1/metrics",
      "http://collector:4318",
    ]) {
      expect(validateOtelSettings({ ...base, endpoint })).toEqual([]);
    }
    expect(validateOtelSettings({ ...base, resourceAttributes: { ["a".repeat(64)]: "v" } })).toEqual([]);
    expect(validateOtelSettings(base)).toEqual([]);
  });

  test("describes OTel coverage as off, healthy, and failing", () => {
    expect(describeOtelCoverage({ enabled: false })).toBe("OTel — off");
    expect(describeOtelCoverage({
      enabled: true,
      endpoint: "http://collector:4318/v1/metrics",
      intervalSec: 60,
      lastSuccessMs: Date.UTC(2026, 7, 29, 10, 11, 12),
      lastFailureMs: null,
      failures: 0,
    })).toContain(
      `OTel → http://collector:4318/v1/metrics every 60 s · last success ${new Date(Date.UTC(2026, 7, 29, 10, 11, 12)).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false,
      })} · failures 0`,
    );
    expect(describeOtelCoverage({
      enabled: true,
      endpoint: "http://collector:4318/v1/metrics",
      intervalSec: 5,
      lastSuccessMs: Date.UTC(2026, 7, 29, 10, 11, 12),
      lastFailureMs: Date.UTC(2026, 7, 29, 10, 11, 13),
      failures: 1,
      lastError: "connection refused",
    })).toContain(" · last error: connection refused");
  });

  test("settings payload third argument controls OTel omission", () => {
    const settings = { otel: { enabled: true }, retentionLadder: { l1: { keepDays: 3 } } };
    expect(settingsPutPayload(settings, true, false)).toEqual({ retentionLadder: settings.retentionLadder });
    expect(settingsPutPayload(settings, true, true)).toEqual(settings);
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

  test("names a zero-impact ladder change on the import path", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 12;

    expect(
      describeImportPlan(
        {
          wouldDelete: {
            l1Rows: 0,
            l2Buckets: 0,
            l3Buckets: 0,
            l4Buckets: 0,
            snapshotJsonRows: 0,
          },
          changedKeys: ["retentionLadder", "defaultTheme"],
          warnings: [],
        },
        candidate,
        { retentionLadder: previous },
      ),
    ).toEqual([
      "retention ladder changes — no stored history is affected",
      "also changes: defaultTheme",
    ]);
  });

  test("keeps a save preview silent for a zero-impact ladder change", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 12;
    const plan = {
      wouldDelete: {
        l1Rows: 0,
        l2Buckets: 0,
        l3Buckets: 0,
        l4Buckets: 0,
        snapshotJsonRows: 0,
        processFastRows: 0,
      },
      changedKeys: ["retentionLadder", "defaultTheme"],
      warnings: [],
    };

    expect(
      describeImportPlan(plan, candidate, { retentionLadder: previous }, { includeOtherChanges: false }),
    ).toEqual([]);
  });

  test("does not add the ladder line when a count already describes it", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l2.keepDays = 12;

    expect(
      describeImportPlan(
        {
          wouldDelete: { l2Buckets: 5 },
          changedKeys: ["retentionLadder", "defaultTheme"],
          warnings: [],
        },
        candidate,
        { retentionLadder: previous },
      ),
    ).toEqual(["5 L2 buckets", "also changes: defaultTheme"]);
  });

  test("does not add the ladder line when a transition describes it", () => {
    const previous = ladder();
    const candidate = ladder();
    candidate.l3.enabled = false;

    expect(
      describeImportPlan(
        {
          wouldDelete: {
            l1Rows: 0,
            l2Buckets: 0,
            l3Buckets: 0,
            l4Buckets: 0,
            snapshotJsonRows: 0,
          },
          changedKeys: ["retentionLadder"],
          warnings: [],
        },
        candidate,
        { retentionLadder: previous },
      ),
    ).toEqual(["L3 disabled — its table is retained; reads fall through to the next tier"]);
  });

  test("describes an identical document", () => {
    expect(
      describeImportPlan(
        {
          wouldDelete: {
            l1Rows: 0,
            l2Buckets: 0,
            l3Buckets: 0,
            l4Buckets: 0,
            snapshotJsonRows: 0,
          },
          changedKeys: [],
          warnings: [],
        },
        ladder(),
        { retentionLadder: ladder() },
      ),
    ).toEqual(["no settings change — the document matches the current settings"]);
  });

  test("stays silent on the save path for an identical document", () => {
    const plan = {
      wouldDelete: {
        l1Rows: 0,
        l2Buckets: 0,
        l3Buckets: 0,
        l4Buckets: 0,
        snapshotJsonRows: 0,
        processFastRows: 0,
      },
      changedKeys: [],
      warnings: [],
    };

    expect(
      describeImportPlan(plan, ladder(), { retentionLadder: ladder() }, { includeOtherChanges: false }),
    ).toEqual([]);
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
            processFastRows: 3,
          },
          changedKeys: ["retentionLadder", "pollIntervalMs"],
          warnings: ["settings.bogus: unknown key ignored"],
        },
        candidate,
        { retentionLadder: ladder() },
      ),
    ).toEqual([
      `${(1234).toLocaleString()} L1 rows`,
      "56 L2 buckets",
      "7 L3 buckets",
      "8 L4 buckets (moved to the queryable archive)",
      "90 snapshot JSON blobs stripped",
      "3 fast process rows deleted",
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
        processFastRows: 0,
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
