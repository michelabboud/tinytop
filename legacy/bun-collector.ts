import { collectSnapshot, type SystemSnapshot } from "../src/collector";
import {
  DEFAULT_HISTORY_LIMIT,
  defaultHistoryDbPath,
  openHistoryStore,
  type HistoryQuery,
  type HistorySample,
  type HistoryStore,
} from "../src/history-store";

const DEFAULT_WRITER_HOST = "127.0.0.1";
const DEFAULT_WRITER_PORT = 4276;
const DEFAULT_POLL_MS = 1500;
const DEFAULT_WINDOW_SECONDS = 300;

type SnapshotResult = {
  snapshot: SystemSnapshot;
  currentProcStatText: string;
};

type CollectorHandlerOptions = {
  store: HistoryStore;
  collect?: (previousProcStatText?: string) => Promise<SnapshotResult>;
  now?: () => number;
};

function jsonError(message: string, status: number): Response {
  return Response.json({ error: message }, { status });
}

function parsePositiveInteger(value: string | null, fallback: number): number {
  if (value === null) return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(1, Math.floor(parsed)) : fallback;
}

function parseHistoryQuery(searchParams: URLSearchParams, now: () => number): HistoryQuery {
  const limit = parsePositiveInteger(searchParams.get("limit"), DEFAULT_HISTORY_LIMIT);
  const untilMs = searchParams.has("until_ms") ? Number(searchParams.get("until_ms")) : undefined;
  const explicitSinceMs = searchParams.has("since_ms") ? Number(searchParams.get("since_ms")) : undefined;
  const windowSeconds = parsePositiveInteger(searchParams.get("window_seconds"), DEFAULT_WINDOW_SECONDS);
  const sinceMs = Number.isFinite(explicitSinceMs) ? explicitSinceMs : now() - windowSeconds * 1000;

  return {
    sinceMs,
    untilMs: Number.isFinite(untilMs) ? untilMs : undefined,
    limit,
  };
}

export function createCollectorFetchHandler(options: CollectorHandlerOptions): (request: Request) => Promise<Response> {
  let previousProcStatText: string | undefined;
  let collectPromise: Promise<HistorySample> | null = null;
  const collect = options.collect ?? collectSnapshot;
  const now = options.now ?? Date.now;

  async function collectAndStore(): Promise<HistorySample> {
    if (collectPromise) return collectPromise;

    collectPromise = collect(previousProcStatText)
      .then((result) => {
        previousProcStatText = result.currentProcStatText;
        return options.store.insertSnapshot(result.snapshot);
      })
      .finally(() => {
        collectPromise = null;
      });

    return collectPromise;
  }

  return async function handleCollectorRequest(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (request.method !== "GET") {
      return jsonError("Only GET requests are supported", 405);
    }

    if (url.pathname === "/health") {
      return new Response("ok", {
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }

    if (url.pathname === "/snapshot/latest") {
      try {
        const latest = options.store.latestSnapshot() ?? (await collectAndStore());
        return Response.json(latest.snapshot, {
          headers: { "cache-control": "no-store" },
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : "latest snapshot failed";
        return jsonError(message, 500);
      }
    }

    if (url.pathname === "/snapshot/collect") {
      try {
        const sample = await collectAndStore();
        return Response.json(sample.snapshot, {
          headers: { "cache-control": "no-store" },
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : "snapshot collection failed";
        return jsonError(message, 500);
      }
    }

    if (url.pathname === "/history") {
      try {
        const query = parseHistoryQuery(url.searchParams, now);
        const samples = options.store.readHistory(query);
        return Response.json(
          { samples },
          {
            headers: { "cache-control": "no-store" },
          },
        );
      } catch (error) {
        const message = error instanceof Error ? error.message : "history query failed";
        return jsonError(message, 500);
      }
    }

    return jsonError("Collector route not found", 404);
  };
}

export const createWriterFetchHandler = createCollectorFetchHandler;

export function startLegacyBunCollector(): { url: string; stop(force?: boolean): void } {
  const hostname = process.env.HISTORY_WRITER_HOST ?? DEFAULT_WRITER_HOST;
  const port = Number(process.env.HISTORY_WRITER_PORT ?? DEFAULT_WRITER_PORT);
  const pollMs = Number(process.env.HISTORY_POLL_MS ?? DEFAULT_POLL_MS);
  const store = openHistoryStore(defaultHistoryDbPath());
  const fetch = createCollectorFetchHandler({ store });

  const server = Bun.serve({
    hostname,
    port,
    fetch,
  });

  const collectTimer = setInterval(async () => {
    try {
      await fetch(new Request(`${server.url}snapshot/collect`));
    } catch (error) {
      console.error(error instanceof Error ? error.message : "scheduled collection failed");
    }
  }, pollMs);

  fetch(new Request(`${server.url}snapshot/collect`)).catch((error) => {
    console.error(error instanceof Error ? error.message : "initial collection failed");
  });

  console.log(`TinyTop legacy Bun collector listening on ${server.url}`);
  console.log(`History database: ${defaultHistoryDbPath()}`);

  return {
    url: String(server.url),
    stop(force?: boolean) {
      clearInterval(collectTimer);
      store.close();
      server.stop(force);
    },
  };
}

export const startCollectorDaemon = startLegacyBunCollector;

async function runCheck(): Promise<void> {
  const store = openHistoryStore(":memory:");
  const { snapshot } = await collectSnapshot();
  const sample = store.insertSnapshot(snapshot);
  const latest = store.latestSnapshot();
  store.close();

  console.log(
    JSON.stringify(
      {
        status: latest ? "ok" : "error",
        capturedAtMs: sample.capturedAtMs,
        runtime: snapshot.identity.runtime.kind,
        hostname: snapshot.identity.hostname,
        db: "memory",
      },
      null,
      2,
    ),
  );
}

if (process.argv[1]?.endsWith("legacy/bun-collector.ts")) {
  if (process.argv.includes("--check")) {
    await runCheck();
  } else {
    startLegacyBunCollector();
  }
}
