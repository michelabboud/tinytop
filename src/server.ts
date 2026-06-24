import { collectSnapshot, type SystemSnapshot } from "./collector";
import type { HistoryQuery, HistorySample } from "./history-store";

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 4274;
const DEFAULT_WRITER_HOST = "127.0.0.1";
const DEFAULT_WRITER_PORT = 4276;
const PUBLIC_DIR = new URL("../public", import.meta.url).pathname;
const ECHARTS_VENDOR = new URL("../public/vendor/echarts.min.js", import.meta.url).pathname;

type SnapshotResult = {
  snapshot: SystemSnapshot;
  currentProcStatText: string;
};

type FetchHandlerOptions = {
  publicDir: string;
  collect?: (previousProcStatText?: string) => Promise<SnapshotResult>;
  readHistory?: (query: HistoryQuery) => Promise<HistorySample[]> | HistorySample[];
  writerFetch?: (pathnameWithSearch: string) => Promise<Response>;
};

const contentTypes = new Map<string, string>([
  ["html", "text/html; charset=utf-8"],
  ["css", "text/css; charset=utf-8"],
  ["js", "text/javascript; charset=utf-8"],
  ["svg", "image/svg+xml"],
  ["json", "application/json; charset=utf-8"],
]);

function jsonError(message: string, status: number): Response {
  return Response.json({ error: message }, { status });
}

function contentTypeFor(pathname: string): string {
  const extension = pathname.split(".").pop() ?? "";
  return contentTypes.get(extension) ?? "application/octet-stream";
}

function staticFileName(pathname: string): string | null {
  if (pathname === "/") return "index.html";
  if (pathname === "/index.html") return "index.html";
  if (pathname === "/styles.css") return "styles.css";
  if (pathname === "/app.js") return "app.js";
  return null;
}

function staticFilePath(pathname: string, publicDir: string): string | null {
  if (pathname === "/vendor/echarts.min.js") {
    return ECHARTS_VENDOR;
  }
  const fileName = staticFileName(pathname);
  return fileName ? `${publicDir}/${fileName}` : null;
}

function parseOptionalNumber(value: string | null): number | undefined {
  if (value === null) return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function historyQueryFromUrl(url: URL): HistoryQuery {
  const windowSeconds = parseOptionalNumber(url.searchParams.get("window_seconds"));
  const explicitSinceMs = parseOptionalNumber(url.searchParams.get("since_ms"));
  const sinceMs = explicitSinceMs ?? (windowSeconds ? Date.now() - windowSeconds * 1000 : undefined);

  return {
    sinceMs,
    untilMs: parseOptionalNumber(url.searchParams.get("until_ms")),
    limit: parseOptionalNumber(url.searchParams.get("limit")),
  };
}

async function copyJsonResponse(response: Response): Promise<Response> {
  return new Response(await response.text(), {
    status: response.status,
    headers: {
      "content-type": response.headers.get("content-type") ?? "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

async function fetchWriterWithRetry(writerBaseUrl: string, pathnameWithSearch: string): Promise<Response> {
  const url = new URL(pathnameWithSearch, writerBaseUrl);
  let lastError: unknown;

  for (let attempt = 0; attempt < 10; attempt += 1) {
    try {
      return await globalThis.fetch(url);
    } catch (error) {
      lastError = error;
      await Bun.sleep(100);
    }
  }

  throw lastError instanceof Error ? lastError : new Error("writer process is unavailable");
}

export function createFetchHandler(options: FetchHandlerOptions): (request: Request) => Promise<Response> {
  let previousProcStatText: string | undefined;

  return async function handleRequest(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (request.method !== "GET") {
      return jsonError("Only GET requests are supported", 405);
    }

    if (url.pathname === "/health") {
      return new Response("ok", {
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }

    if (url.pathname === "/api/snapshot") {
      try {
        if (options.writerFetch) {
          return copyJsonResponse(await options.writerFetch("/snapshot/latest"));
        }

        if (!options.collect) return jsonError("Snapshot provider is not configured", 503);
        const result = await options.collect(previousProcStatText);
        previousProcStatText = result.currentProcStatText;
        return Response.json(result.snapshot, {
          headers: {
            "cache-control": "no-store",
          },
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : "snapshot collection failed";
        return jsonError(message, 500);
      }
    }

    if (url.pathname === "/api/history") {
      try {
        if (options.writerFetch) {
          return copyJsonResponse(await options.writerFetch(`/history${url.search}`));
        }

        if (!options.readHistory) return jsonError("History provider is not configured", 503);
        const samples = await options.readHistory(historyQueryFromUrl(url));
        return Response.json(
          { samples },
          {
            headers: {
              "cache-control": "no-store",
            },
          },
        );
      } catch (error) {
        const message = error instanceof Error ? error.message : "history query failed";
        return jsonError(message, 500);
      }
    }

    if (url.pathname.startsWith("/api/")) {
      return jsonError("API route not found", 404);
    }

    const filePath = staticFilePath(url.pathname, options.publicDir);
    if (!filePath) {
      return new Response("Not found", { status: 404 });
    }

    return new Response(Bun.file(filePath), {
      headers: {
        "content-type": contentTypeFor(filePath),
        "cache-control": "no-store",
      },
    });
  };
}

export function startServer(): { url: string; stop(force?: boolean): void } {
  const port = Number(process.env.PORT ?? DEFAULT_PORT);
  const hostname = process.env.HOST ?? DEFAULT_HOST;
  const writerBaseUrl =
    process.env.HISTORY_WRITER_URL ?? `http://${process.env.HISTORY_WRITER_HOST ?? DEFAULT_WRITER_HOST}:${process.env.HISTORY_WRITER_PORT ?? DEFAULT_WRITER_PORT}`;
  const writerProcess =
    process.env.HISTORY_WRITER_URL || process.env.TINYTOP_DISABLE_WRITER_SPAWN === "1"
      ? null
      : Bun.spawn(["bun", "run", new URL("./collector-daemon.ts", import.meta.url).pathname], {
          stdout: "inherit",
          stderr: "inherit",
          env: {
            ...process.env,
            HISTORY_WRITER_HOST: process.env.HISTORY_WRITER_HOST ?? DEFAULT_WRITER_HOST,
            HISTORY_WRITER_PORT: process.env.HISTORY_WRITER_PORT ?? String(DEFAULT_WRITER_PORT),
          },
        });
  const fetchHandler = createFetchHandler({
    publicDir: PUBLIC_DIR,
    writerFetch: (pathnameWithSearch) => fetchWriterWithRetry(writerBaseUrl, pathnameWithSearch),
  });

  const server = Bun.serve({
    hostname,
    port,
    fetch: fetchHandler,
  });

  console.log(`TinyTop listening on ${server.url}`);
  return {
    url: String(server.url),
    stop(force?: boolean) {
      server.stop(force);
      writerProcess?.kill();
    },
  };
}

async function runCheck(): Promise<void> {
  const { snapshot } = await collectSnapshot();
  console.log(
    JSON.stringify(
      {
        status: "ok",
        runtime: snapshot.identity.runtime.kind,
        hostname: snapshot.identity.hostname,
        cpuPercent: snapshot.cpu.usagePercent,
        memoryPercent: snapshot.memory.usedPercent,
      },
      null,
      2,
    ),
  );
}

if (process.argv[1]?.endsWith("src/server.ts")) {
  if (process.argv.includes("--check")) {
    await runCheck();
  } else {
    startServer();
  }
}
