import { afterEach, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openHistoryStore } from "../src/history-store";
import { makeSnapshot } from "./fixtures";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function nullableV1DatabasePath(): string {
  const dir = mkdtempSync(join(tmpdir(), "tinytop-history-null-json-"));
  tempDirs.push(dir);
  const dbPath = join(dir, "history.sqlite");
  const db = new Database(dbPath, { create: true });
  db.exec(`
    CREATE TABLE metric_samples (
      sample_id INTEGER PRIMARY KEY,
      captured_at_ms INTEGER NOT NULL UNIQUE,
      snapshot_timestamp TEXT NOT NULL,
      hostname TEXT NOT NULL,
      runtime_kind TEXT NOT NULL,
      cpu_usage_percent REAL NOT NULL,
      cpu_cores INTEGER NOT NULL,
      memory_used_percent REAL NOT NULL,
      memory_used_bytes INTEGER NOT NULL,
      memory_total_bytes INTEGER NOT NULL,
      swap_used_percent REAL NOT NULL,
      swap_used_bytes INTEGER NOT NULL,
      swap_total_bytes INTEGER NOT NULL,
      load_one REAL NOT NULL,
      load_five REAL NOT NULL,
      load_fifteen REAL NOT NULL,
      load_percent REAL NOT NULL,
      runnable_threads INTEGER NOT NULL,
      total_threads INTEGER NOT NULL,
      root_used_percent REAL,
      snapshot_json TEXT
    );
  `);
  db.close();
  return dbPath;
}

test("raw reads exclude v1 rows whose snapshot JSON was pruned", () => {
  // Break caught: Bun returns a migrated row with a null snapshot instead of
  // limiting raw history to the snapshot JSON retention window.
  const dbPath = nullableV1DatabasePath();
  const older = makeSnapshot({ timestamp: "2026-08-28T12:00:00.000Z" });
  const newer = makeSnapshot({ timestamp: "2026-08-28T12:01:00.000Z" });

  const writer = openHistoryStore(dbPath);
  writer.insertSnapshot(older);
  writer.insertSnapshot(newer);
  writer.close();

  const raw = new Database(dbPath);
  raw
    .query("UPDATE metric_samples SET snapshot_json = NULL WHERE captured_at_ms = ?")
    .run(Date.parse(older.timestamp));
  raw.close();

  const reader = openHistoryStore(dbPath);
  const history = reader.readHistory();
  const latest = reader.latestSnapshot();
  reader.close();

  expect(history).toHaveLength(1);
  expect(history[0]?.capturedAtMs).toBe(Date.parse(newer.timestamp));
  expect(latest?.capturedAtMs).toBe(Date.parse(newer.timestamp));
});
