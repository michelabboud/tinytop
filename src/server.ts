import { collectSnapshot, type SystemSnapshot } from "./collector";

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 4274;
const PUBLIC_DIR = new URL("../public", import.meta.url).pathname;
const ECHARTS_DIST = new URL("../node_modules/echarts/dist/echarts.min.js", import.meta.url).pathname;

type SnapshotResult = {
  snapshot: SystemSnapshot;
  currentProcStatText: string;
};

type FetchHandlerOptions = {
  publicDir: string;
  collect: (previousProcStatText?: string) => Promise<SnapshotResult>;
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
  if (pathname === "/vendor/echarts.min.js") return ECHARTS_DIST;
  const fileName = staticFileName(pathname);
  return fileName ? `${publicDir}/${fileName}` : null;
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
  const fetch = createFetchHandler({
    publicDir: PUBLIC_DIR,
    collect: collectSnapshot,
  });

  const server = Bun.serve({
    hostname,
    port,
    fetch,
  });

  console.log(`WSL Status Dashboard listening on ${server.url}`);
  return server;
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
