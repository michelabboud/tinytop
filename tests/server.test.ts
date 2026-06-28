import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { createFetchHandler } from "../src/server";
import type { SystemSnapshot } from "../src/collector";
import { createCollectorFetchHandler } from "../legacy/bun-collector";
import { makeSnapshot } from "./fixtures";

const productVersion = readFileSync("VERSION", "utf8").trim();

const snapshot: SystemSnapshot = {
  timestamp: "2026-06-24T12:00:00.000Z",
  identity: {
    hostname: "devbox",
    platform: "linux",
    arch: "x64",
    distro: "Ubuntu 24.04.2 LTS",
    kernel: "5.15.167.4-microsoft-standard-WSL2",
    runtime: {
      kind: "WSL",
      confidence: "high",
      reason: "kernel release/version contains Microsoft WSL markers",
    },
    uptimeSeconds: 123,
  },
  cpu: {
    usagePercent: 42,
    cores: 8,
    times: {
      user: 1,
      nice: 0,
      system: 1,
      idle: 8,
      iowait: 0,
      irq: 0,
      softirq: 0,
      steal: 0,
      guest: 0,
      guestNice: 0,
      total: 10,
      idleTotal: 8,
    },
  },
  memory: {
    totalBytes: 100,
    availableBytes: 40,
    usedBytes: 60,
    usedPercent: 60,
  },
  swap: {
    totalBytes: 100,
    freeBytes: 75,
    usedBytes: 25,
    usedPercent: 25,
  },
  load: {
    one: 1,
    five: 2,
    fifteen: 3,
    runnable: 1,
    totalThreads: 100,
    lastPid: 999,
  },
  pressure: {
    cpu: {},
    memory: {},
    io: {},
  },
  filesystems: [],
  processes: [],
};

describe("createFetchHandler", () => {
  test("responds to health checks", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });

    const response = await handler(new Request("http://127.0.0.1:4274/health"));
    const body = await response.json();

    expect(response.status).toBe(200);
    expect(body.status).toBe("ok");
    expect(body.daemon.os).toBe(process.platform);
    expect(body.daemon.install.workingDirectory).toBe(process.cwd());
  });

  test("serves legacy dashboard version metadata", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      writerFetch: async (pathnameWithSearch) => {
        if (pathnameWithSearch === "/version") {
          return Response.json({
            version: productVersion,
            runtime: "legacy-bun",
            component: "collector",
            dashboard: "none",
          });
        }
        return new Response("not found", { status: 404 });
      },
    });

    const response = await handler(new Request("http://127.0.0.1:4274/api/version"));
    const body = await response.json();

    expect(response.status).toBe(200);
    expect(body.version).toBe(productVersion);
    expect(body.runtime).toBe("legacy-bun");
    expect(body.component).toBe("dashboard");
    expect(body.dashboard).toBe("legacy");
    expect(body.collector.component).toBe("collector");
    expect(body.daemon.os).toBe(process.platform);
    expect(body.daemon.storage.sqlitePath).toContain("history.sqlite");
  });

  test("serves legacy dashboard settings metadata", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });

    const defaultResponse = await handler(new Request("http://127.0.0.1:4274/api/settings"));
    const defaults = await defaultResponse.json();

    expect(defaultResponse.status).toBe(200);
    expect(defaults.defaultTheme).toBe("midnight");
    expect(defaults.pollIntervalMs).toBe(1500);
    expect(defaults.thresholds.cpuCritical).toBe(95);
    expect(defaults.thresholds.loadWarn).toBe(80);
    expect(defaults.thresholds.pressureCritical).toBe(25);

    const updateResponse = await handler(
      new Request("http://127.0.0.1:4274/api/settings", {
        method: "PUT",
        body: JSON.stringify({
          ...defaults,
          defaultTheme: "aurora",
          defaultGraphMode: "heatmap",
          pollIntervalMs: 3000,
          thresholds: {
            ...defaults.thresholds,
            cpuWarn: 70,
            cpuCritical: 90,
            loadWarn: 75,
            loadCritical: 95,
          },
        }),
      }),
    );
    const updated = await updateResponse.json();

    expect(updateResponse.status).toBe(200);
    expect(updated.defaultTheme).toBe("aurora");
    expect(updated.defaultGraphMode).toBe("heatmap");
    expect(updated.pollIntervalMs).toBe(3000);
    expect(updated.thresholds.cpuWarn).toBe(70);
    expect(updated.thresholds.cpuCritical).toBe(90);
    expect(updated.thresholds.loadWarn).toBe(75);
    expect(updated.thresholds.loadCritical).toBe(95);
  });

  test("serves the live snapshot JSON API", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });

    const response = await handler(new Request("http://127.0.0.1:4274/api/snapshot"));
    const body = await response.json();

    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toContain("application/json");
    expect(body.identity.runtime.kind).toBe("WSL");
    expect(body.cpu.usagePercent).toBe(42);
  });

  test("serves persisted history samples for dashboard hydration", async () => {
    const persisted = makeSnapshot({ timestamp: "2026-06-24T12:00:01.500Z" });
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
      readHistory: async () => [{ capturedAtMs: Date.parse(persisted.timestamp), snapshot: persisted }],
    });

    const response = await handler(new Request("http://127.0.0.1:4274/api/history?limit=20&window_seconds=300"));
    const body = await response.json();

    expect(response.status).toBe(200);
    expect(body.samples).toHaveLength(1);
    expect(body.samples[0].capturedAtMs).toBe(Date.parse(persisted.timestamp));
    expect(body.samples[0].snapshot.timestamp).toBe(persisted.timestamp);
  });

  test("serves the local ECharts browser bundle", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });

    const response = await handler(new Request("http://127.0.0.1:4274/vendor/echarts.min.js"));
    const body = await response.text();

    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toContain("text/javascript");
    expect(body).toContain("echarts");
  });

  test("returns 404 for unknown API routes", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });

    const response = await handler(new Request("http://127.0.0.1:4274/api/nope"));

    expect(response.status).toBe(404);
  });
});

describe("createCollectorFetchHandler", () => {
  test("serves legacy collector version metadata", async () => {
    const handler = createCollectorFetchHandler({
      store: {
        insertSnapshot: (nextSnapshot) => ({ capturedAtMs: Date.parse(nextSnapshot.timestamp), snapshot: nextSnapshot }),
        latestSnapshot: () => null,
        readHistory: () => [],
        close: () => {},
      },
      collect: async () => ({ snapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });

    const response = await handler(new Request("http://127.0.0.1:4276/version"));
    const body = await response.json();

    expect(response.status).toBe(200);
    expect(body.version).toBe(productVersion);
    expect(body.runtime).toBe("legacy-bun");
    expect(body.component).toBe("collector");
    expect(body.dashboard).toBe("none");
  });
});
