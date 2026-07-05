import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const repoRoot = new URL("..", import.meta.url).pathname;

const dashboardFiles = [
  "index.html",
  "favicon.svg",
  "styles.css",
  "app.js",
  "vendor/echarts.min.js",
  "vendor/echarts.LICENSE",
  "vendor/echarts.LICENSE-d3",
  "vendor/echarts.NOTICE",
];

function read(path: string): Buffer {
  return readFileSync(join(repoRoot, path));
}

describe("dashboard asset ownership", () => {
  test("agent/assets/dashboard is the single dashboard source (no duplicate trees)", () => {
    // Both runtimes consume ONE dashboard: the Rust agent embeds
    // agent/assets/dashboard/* at compile time (include_bytes!) and the Bun
    // server serves the same directory from disk. The former legacy/dashboard
    // duplicate (kept byte-identical by a test) is gone — a byte-identity test
    // is a workaround for shared code, not shared code.
    for (const file of dashboardFiles) {
      expect(existsSync(join(repoRoot, `agent/assets/dashboard/${file}`))).toBe(true);
    }
    expect(existsSync(join(repoRoot, "legacy/dashboard"))).toBe(false);
    expect(existsSync(join(repoRoot, "public/index.html"))).toBe(false);
    expect(existsSync(join(repoRoot, "public/app.js"))).toBe(false);
    expect(existsSync(join(repoRoot, "public/styles.css"))).toBe(false);
  });

  test("dashboard declares a served SVG favicon", () => {
    const html = read("agent/assets/dashboard/index.html").toString("utf8");
    const favicon = read("agent/assets/dashboard/favicon.svg").toString("utf8");

    expect(html).toContain('<link rel="icon" type="image/svg+xml" href="favicon.svg" />');
    expect(favicon).toContain("<svg");
    expect(favicon).toContain("TinyTop");
  });

  test("dashboard asset refs are reverse-proxy-embeddable (base-relative, not root-absolute)", () => {
    // The embeddable /embed view is served by tutus-remotus at a sub-path
    // (/proxy/{id}/embed). Root-absolute asset URLs (href="/styles.css") would
    // resolve against the proxy ORIGIN root and miss the tunnel; base-relative
    // URLs resolve under the sub-path and are stripped back correctly. Standalone
    // is unaffected. Lock this so a future edit can't silently re-break the embed.
    const html = read("agent/assets/dashboard/index.html").toString("utf8");
    expect(html).toContain('href="favicon.svg"');
    expect(html).toContain('href="styles.css"');
    expect(html).toContain('src="app.js"');
    expect(html).toContain('src="vendor/echarts.min.js"');
    // No root-absolute same-origin asset references.
    expect(html).not.toContain('href="/styles.css"');
    expect(html).not.toContain('href="/favicon.svg"');
    expect(html).not.toContain('src="/app.js"');
    expect(html).not.toContain('src="/vendor/echarts.min.js"');
  });
});

describe("dashboard API sub-path derivation", () => {
  // Evaluate the SHIPPED dashboardBasePath from app.js (not a test replica), so
  // these cases exercise the exact code both runtimes serve. The function must
  // stay pure (pathname in, prefix out) for this extraction to keep working.
  function loadDashboardBasePath(): (pathname: string) => string {
    const source = read("agent/assets/dashboard/app.js").toString("utf8");
    const match = source.match(/function dashboardBasePath\(pathname\) \{[\s\S]*?\n\}/);
    if (!match) throw new Error("dashboardBasePath(pathname) not found in app.js");
    return new Function(`${match[0]}; return dashboardBasePath;`)() as (
      pathname: string,
    ) => string;
  }

  test("API calls resolve under any mount, not only the /embed leaf", () => {
    const basePath = loadDashboardBasePath();
    const cases: Array<[string, string]> = [
      // root mounts — no prefix
      ["/", ""],
      ["/index.html", ""],
      ["/embed", ""],
      // nginx-style standalone sub-path (the /mon regression: API calls used to
      // resolve to the domain root and 404 while assets loaded fine)
      ["/mon/", "/mon"],
      ["/mon", "/mon"],
      ["/mon/index.html", "/mon"],
      ["/mon/embed", "/mon"],
      // tutus-remotus reverse-proxy embed
      ["/proxy/abc123/embed", "/proxy/abc123"],
      // deeper nesting
      ["/ops/hosts/wizai/", "/ops/hosts/wizai"],
    ];
    for (const [pathname, expected] of cases) {
      expect(basePath(pathname), `pathname ${pathname}`).toBe(expected);
    }
  });

  test("apiPath uses the derived base for fetches", () => {
    const source = read("agent/assets/dashboard/app.js").toString("utf8");
    // apiPath must consume dashboardBasePath — not re-derive an /embed-only base.
    expect(source).toContain("dashboardBasePath(DASHBOARD_URL.pathname)");
    expect(source).not.toContain("embedIndex > 0");
  });
});
