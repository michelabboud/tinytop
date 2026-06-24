import { afterEach, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { openHistoryStore } from "../src/history-store";
import { makeSnapshot } from "./fixtures";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function tempDbPath(): string {
  const dir = mkdtempSync(join(tmpdir(), "wsl-status-history-"));
  tempDirs.push(dir);
  return join(dir, "history.sqlite");
}

describe("history store", () => {
  test("persists samples across store instances and returns newest-window history oldest first", () => {
    const dbPath = tempDbPath();
    const first = makeSnapshot({
      timestamp: "2026-06-24T12:00:00.000Z",
      cpu: { ...makeSnapshot().cpu, usagePercent: 10 },
    });
    const second = makeSnapshot({
      timestamp: "2026-06-24T12:00:01.500Z",
      cpu: { ...makeSnapshot().cpu, usagePercent: 20 },
    });

    const writer = openHistoryStore(dbPath);
    writer.insertSnapshot(first);
    writer.insertSnapshot(second);
    writer.close();

    const reader = openHistoryStore(dbPath);
    const samples = reader.readHistory({ limit: 2 });
    const latest = reader.latestSnapshot();
    reader.close();

    expect(samples.map((sample) => sample.snapshot.cpu.usagePercent)).toEqual([10, 20]);
    expect(samples.map((sample) => sample.capturedAtMs)).toEqual([
      Date.parse("2026-06-24T12:00:00.000Z"),
      Date.parse("2026-06-24T12:00:01.500Z"),
    ]);
    expect(latest?.snapshot.cpu.usagePercent).toBe(20);
  });
});
