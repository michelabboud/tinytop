import { afterEach, describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { openHistoryStore } from "../src/history-store";
import {
  checkHistoryDb,
  readHistoryDbStats,
  resolveHistoryDbPath,
  vacuumHistoryDb,
} from "../src/ops";
import { makeSnapshot } from "./fixtures";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function tempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

function tempDbPath(): string {
  return join(tempDir("tinytop-ops-"), "history.sqlite");
}

function writeSampleDb(dbPath: string): void {
  const store = openHistoryStore(dbPath);
  store.insertSnapshot(
    makeSnapshot({
      timestamp: "2026-06-24T12:00:00.000Z",
      cpu: { ...makeSnapshot().cpu, usagePercent: 11 },
    }),
  );
  store.insertSnapshot(
    makeSnapshot({
      timestamp: "2026-06-24T12:00:01.500Z",
      cpu: { ...makeSnapshot().cpu, usagePercent: 22 },
    }),
  );
  store.close();
}

describe("ops helpers", () => {
  test("resolves TINYTOP_HISTORY_DB before XDG_DATA_HOME defaults", () => {
    expect(
      resolveHistoryDbPath({
        env: {
          TINYTOP_HISTORY_DB: "/tmp/custom.sqlite",
          XDG_DATA_HOME: "/tmp/xdg",
          HOME: "/home/demo",
        },
      }),
    ).toBe("/tmp/custom.sqlite");
  });

  test("resolves XDG_DATA_HOME default history path", () => {
    expect(
      resolveHistoryDbPath({
        env: {
          XDG_DATA_HOME: "/tmp/xdg",
          HOME: "/home/demo",
        },
      }),
    ).toBe("/tmp/xdg/tinytop/history.sqlite");
  });

  test("reads sample count and time bounds from the history database", () => {
    const dbPath = tempDbPath();
    writeSampleDb(dbPath);

    const stats = readHistoryDbStats(dbPath);

    expect(stats.exists).toBe(true);
    expect(stats.sampleCount).toBe(2);
    expect(stats.earliestCapturedAtMs).toBe(Date.parse("2026-06-24T12:00:00.000Z"));
    expect(stats.latestCapturedAtMs).toBe(Date.parse("2026-06-24T12:00:01.500Z"));
    expect(stats.sizeBytes).toBeGreaterThan(0);
  });

  test("reports a missing history database without creating it", () => {
    const dbPath = join(tempDir("tinytop-missing-"), "missing.sqlite");

    const stats = readHistoryDbStats(dbPath);

    expect(stats.exists).toBe(false);
    expect(stats.sampleCount).toBe(0);
    expect(existsSync(dbPath)).toBe(false);
  });

  test("reports an existing SQLite file without TinyTop tables as empty", () => {
    const dbPath = tempDbPath();
    new Database(dbPath).close();

    const stats = readHistoryDbStats(dbPath);

    expect(stats.exists).toBe(true);
    expect(stats.sampleCount).toBe(0);
    expect(stats.earliestCapturedAtMs).toBeNull();
    expect(stats.latestCapturedAtMs).toBeNull();
  });

  test("checks database integrity and vacuums an existing history database", () => {
    const dbPath = tempDbPath();
    writeSampleDb(dbPath);

    expect(checkHistoryDb(dbPath)).toEqual({ ok: true, message: "ok" });
    expect(vacuumHistoryDb(dbPath)).toEqual({ ok: true });
  });
});
