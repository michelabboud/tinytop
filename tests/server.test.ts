import { describe, expect, test } from "bun:test";
import { createFetchHandler } from "../src/server";
import type { SystemSnapshot } from "../src/collector";
import { makeSnapshot } from "./fixtures";

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

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");
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
