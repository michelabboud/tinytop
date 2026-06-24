import { Database } from "bun:sqlite";
import { existsSync, statSync } from "node:fs";
import { join } from "node:path";

type EnvMap = Record<string, string | undefined>;

export type ResolveHistoryDbPathOptions = {
  env?: EnvMap;
};

export type HistoryDbStats = {
  path: string;
  exists: boolean;
  sizeBytes: number;
  sampleCount: number;
  earliestCapturedAtMs: number | null;
  latestCapturedAtMs: number | null;
};

export type DbCheckResult = {
  ok: boolean;
  message: string;
};

export type VacuumResult = {
  ok: boolean;
  message?: string;
};

type StatsRow = {
  sample_count: number;
  earliest_captured_at_ms: number | null;
  latest_captured_at_ms: number | null;
};

export function resolveHistoryDbPath(options: ResolveHistoryDbPathOptions = {}): string {
  const env = options.env ?? process.env;
  if (env.TINYTOP_HISTORY_DB) return env.TINYTOP_HISTORY_DB;
  const dataHome = env.XDG_DATA_HOME ?? join(env.HOME ?? ".", ".local", "share");
  return join(dataHome, "tinytop", "history.sqlite");
}

function emptyStats(dbPath: string): HistoryDbStats {
  return {
    path: dbPath,
    exists: false,
    sizeBytes: 0,
    sampleCount: 0,
    earliestCapturedAtMs: null,
    latestCapturedAtMs: null,
  };
}

export function readHistoryDbStats(dbPath = resolveHistoryDbPath()): HistoryDbStats {
  if (!existsSync(dbPath)) return emptyStats(dbPath);

  const sizeBytes = statSync(dbPath).size;
  const db = new Database(dbPath, { readonly: true });
  try {
    const row = db
      .query(
        `
          SELECT
            count(*) AS sample_count,
            min(captured_at_ms) AS earliest_captured_at_ms,
            max(captured_at_ms) AS latest_captured_at_ms
          FROM metric_samples
        `,
      )
      .get() as StatsRow;

    return {
      path: dbPath,
      exists: true,
      sizeBytes,
      sampleCount: row.sample_count,
      earliestCapturedAtMs: row.earliest_captured_at_ms,
      latestCapturedAtMs: row.latest_captured_at_ms,
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : "";
    if (message.includes("no such table: metric_samples")) {
      return {
        path: dbPath,
        exists: true,
        sizeBytes,
        sampleCount: 0,
        earliestCapturedAtMs: null,
        latestCapturedAtMs: null,
      };
    }
    throw error;
  } finally {
    db.close();
  }
}

export function checkHistoryDb(dbPath = resolveHistoryDbPath()): DbCheckResult {
  if (!existsSync(dbPath)) {
    return { ok: false, message: `missing: ${dbPath}` };
  }

  const db = new Database(dbPath, { readonly: true });
  try {
    const rows = db.query("PRAGMA integrity_check").all() as Array<{ integrity_check: string }>;
    const messages = rows.map((row) => row.integrity_check);
    return {
      ok: messages.length === 1 && messages[0] === "ok",
      message: messages.join("\n"),
    };
  } finally {
    db.close();
  }
}

export function vacuumHistoryDb(dbPath = resolveHistoryDbPath()): VacuumResult {
  if (!existsSync(dbPath)) {
    return { ok: false, message: `missing: ${dbPath}` };
  }

  const db = new Database(dbPath);
  try {
    db.exec("VACUUM");
    return { ok: true };
  } catch (error) {
    return {
      ok: false,
      message: error instanceof Error ? error.message : "VACUUM failed",
    };
  } finally {
    db.close();
  }
}

function formatDate(ms: number | null): string {
  return ms === null ? "n/a" : new Date(ms).toISOString();
}

export function formatHistoryDbStats(stats: HistoryDbStats): string {
  return [
    `SQLite: ${stats.path}`,
    `Exists: ${stats.exists ? "yes" : "no"}`,
    `Size: ${stats.sizeBytes} bytes`,
    `Samples: ${stats.sampleCount}`,
    `Earliest: ${formatDate(stats.earliestCapturedAtMs)}`,
    `Latest: ${formatDate(stats.latestCapturedAtMs)}`,
  ].join("\n");
}

export async function runOpsCli(args: string[]): Promise<number> {
  const [first, second] = args;

  if (first === "stats") {
    console.log(formatHistoryDbStats(readHistoryDbStats()));
    return 0;
  }

  if (first === "db" && second === "stats") {
    console.log(formatHistoryDbStats(readHistoryDbStats()));
    return 0;
  }

  if (first === "db" && second === "check") {
    const result = checkHistoryDb();
    console.log(result.message);
    return result.ok ? 0 : 1;
  }

  if (first === "db" && second === "vacuum") {
    const result = vacuumHistoryDb();
    if (result.ok) {
      console.log("VACUUM completed");
      return 0;
    }
    console.error(result.message ?? "VACUUM failed");
    return 1;
  }

  console.error("Usage: bun run src/ops.ts stats | db stats | db check | db vacuum");
  return 1;
}

if (process.argv[1]?.endsWith("src/ops.ts")) {
  const code = await runOpsCli(process.argv.slice(2));
  process.exit(code);
}
