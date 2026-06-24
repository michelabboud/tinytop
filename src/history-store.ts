import { Database } from "bun:sqlite";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import type { SystemSnapshot } from "./collector";

export type HistorySample = {
  capturedAtMs: number;
  snapshot: SystemSnapshot;
};

export type HistoryQuery = {
  sinceMs?: number;
  untilMs?: number;
  limit?: number;
};

type MetricSampleRow = {
  captured_at_ms: number;
  snapshot_json: string;
};

export const DEFAULT_HISTORY_LIMIT = 120;

export function defaultHistoryDbPath(): string {
  if (process.env.TINYTOP_HISTORY_DB) return process.env.TINYTOP_HISTORY_DB;
  const dataHome = process.env.XDG_DATA_HOME ?? join(process.env.HOME ?? ".", ".local", "share");
  return join(dataHome, "tinytop", "history.sqlite");
}

function ensureParentDirectory(dbPath: string): void {
  if (dbPath === ":memory:") return;
  mkdirSync(dirname(dbPath), { recursive: true });
}

function capturedAtMsFor(snapshot: SystemSnapshot): number {
  const parsed = Date.parse(snapshot.timestamp);
  return Number.isFinite(parsed) ? parsed : Date.now();
}

function loadPercent(snapshot: SystemSnapshot): number {
  return Math.min(100, (snapshot.load.one / Math.max(1, snapshot.cpu.cores)) * 100);
}

function normalizeLimit(limit: number | undefined): number {
  if (!Number.isFinite(limit)) return DEFAULT_HISTORY_LIMIT;
  return Math.max(1, Math.min(10_000, Math.floor(Number(limit))));
}

function rowToHistorySample(row: MetricSampleRow): HistorySample {
  return {
    capturedAtMs: row.captured_at_ms,
    snapshot: JSON.parse(row.snapshot_json) as SystemSnapshot,
  };
}

export type HistoryStore = {
  insertSnapshot(snapshot: SystemSnapshot): HistorySample;
  latestSnapshot(): HistorySample | null;
  readHistory(query?: HistoryQuery): HistorySample[];
  close(): void;
};

export function openHistoryStore(dbPath = defaultHistoryDbPath()): HistoryStore {
  ensureParentDirectory(dbPath);
  const db = new Database(dbPath, { create: true });

  db.exec("PRAGMA journal_mode = WAL");
  db.exec("PRAGMA synchronous = NORMAL");
  db.exec("PRAGMA busy_timeout = 5000");
  db.exec("PRAGMA foreign_keys = ON");
  db.exec(`
    CREATE TABLE IF NOT EXISTS metric_samples (
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
      snapshot_json TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
      ON metric_samples (captured_at_ms DESC);

    CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
      ON metric_samples (runtime_kind, captured_at_ms DESC);
  `);

  const insertSample = db.prepare(`
    INSERT INTO metric_samples (
      captured_at_ms,
      snapshot_timestamp,
      hostname,
      runtime_kind,
      cpu_usage_percent,
      cpu_cores,
      memory_used_percent,
      memory_used_bytes,
      memory_total_bytes,
      swap_used_percent,
      swap_used_bytes,
      swap_total_bytes,
      load_one,
      load_five,
      load_fifteen,
      load_percent,
      runnable_threads,
      total_threads,
      root_used_percent,
      snapshot_json
    ) VALUES (
      $capturedAtMs,
      $snapshotTimestamp,
      $hostname,
      $runtimeKind,
      $cpuUsagePercent,
      $cpuCores,
      $memoryUsedPercent,
      $memoryUsedBytes,
      $memoryTotalBytes,
      $swapUsedPercent,
      $swapUsedBytes,
      $swapTotalBytes,
      $loadOne,
      $loadFive,
      $loadFifteen,
      $loadPercent,
      $runnableThreads,
      $totalThreads,
      $rootUsedPercent,
      $snapshotJson
    )
    ON CONFLICT(captured_at_ms) DO UPDATE SET
      snapshot_timestamp = excluded.snapshot_timestamp,
      hostname = excluded.hostname,
      runtime_kind = excluded.runtime_kind,
      cpu_usage_percent = excluded.cpu_usage_percent,
      cpu_cores = excluded.cpu_cores,
      memory_used_percent = excluded.memory_used_percent,
      memory_used_bytes = excluded.memory_used_bytes,
      memory_total_bytes = excluded.memory_total_bytes,
      swap_used_percent = excluded.swap_used_percent,
      swap_used_bytes = excluded.swap_used_bytes,
      swap_total_bytes = excluded.swap_total_bytes,
      load_one = excluded.load_one,
      load_five = excluded.load_five,
      load_fifteen = excluded.load_fifteen,
      load_percent = excluded.load_percent,
      runnable_threads = excluded.runnable_threads,
      total_threads = excluded.total_threads,
      root_used_percent = excluded.root_used_percent,
      snapshot_json = excluded.snapshot_json
  `);

  const insertInTransaction = db.transaction((snapshot: SystemSnapshot) => {
    const capturedAtMs = capturedAtMsFor(snapshot);
    const rootFs = snapshot.filesystems.find((filesystem) => filesystem.mount === "/") ?? snapshot.filesystems[0];

    insertSample.run({
      $capturedAtMs: capturedAtMs,
      $snapshotTimestamp: snapshot.timestamp,
      $hostname: snapshot.identity.hostname,
      $runtimeKind: snapshot.identity.runtime.kind,
      $cpuUsagePercent: snapshot.cpu.usagePercent,
      $cpuCores: snapshot.cpu.cores,
      $memoryUsedPercent: snapshot.memory.usedPercent,
      $memoryUsedBytes: snapshot.memory.usedBytes,
      $memoryTotalBytes: snapshot.memory.totalBytes,
      $swapUsedPercent: snapshot.swap.usedPercent,
      $swapUsedBytes: snapshot.swap.usedBytes,
      $swapTotalBytes: snapshot.swap.totalBytes,
      $loadOne: snapshot.load.one,
      $loadFive: snapshot.load.five,
      $loadFifteen: snapshot.load.fifteen,
      $loadPercent: loadPercent(snapshot),
      $runnableThreads: snapshot.load.runnable,
      $totalThreads: snapshot.load.totalThreads,
      $rootUsedPercent: rootFs?.usedPercent ?? null,
      $snapshotJson: JSON.stringify(snapshot),
    });

    return { capturedAtMs, snapshot };
  });

  return {
    insertSnapshot(snapshot) {
      return insertInTransaction(snapshot) as HistorySample;
    },

    latestSnapshot() {
      const row = db
        .query("SELECT captured_at_ms, snapshot_json FROM metric_samples ORDER BY captured_at_ms DESC LIMIT 1")
        .get() as MetricSampleRow | null;
      return row ? rowToHistorySample(row) : null;
    },

    readHistory(query = {}) {
      const limit = normalizeLimit(query.limit);
      const clauses: string[] = [];
      const params: Record<string, number> = { $limit: limit };

      if (Number.isFinite(query.sinceMs)) {
        clauses.push("captured_at_ms >= $sinceMs");
        params.$sinceMs = Number(query.sinceMs);
      }

      if (Number.isFinite(query.untilMs)) {
        clauses.push("captured_at_ms <= $untilMs");
        params.$untilMs = Number(query.untilMs);
      }

      const where = clauses.length > 0 ? `WHERE ${clauses.join(" AND ")}` : "";
      const rows = db
        .query(
          `
            SELECT captured_at_ms, snapshot_json
            FROM metric_samples
            ${where}
            ORDER BY captured_at_ms DESC
            LIMIT $limit
          `,
        )
        .all(params) as MetricSampleRow[];

      return rows.reverse().map(rowToHistorySample);
    },

    close() {
      db.exec("PRAGMA optimize");
      db.close();
    },
  };
}
