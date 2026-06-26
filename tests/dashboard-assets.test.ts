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
  test("legacy dashboard and Rust embedded dashboard assets stay identical", () => {
    for (const file of dashboardFiles) {
      expect(read(`agent/assets/dashboard/${file}`)).toEqual(read(`legacy/dashboard/${file}`));
    }
  });

  test("root public dashboard files moved to legacy ownership", () => {
    expect(existsSync(join(repoRoot, "legacy/dashboard/index.html"))).toBe(true);
    expect(existsSync(join(repoRoot, "agent/assets/dashboard/index.html"))).toBe(true);
    expect(existsSync(join(repoRoot, "public/index.html"))).toBe(false);
    expect(existsSync(join(repoRoot, "public/app.js"))).toBe(false);
    expect(existsSync(join(repoRoot, "public/styles.css"))).toBe(false);
  });

  test("dashboard declares a served SVG favicon", () => {
    const html = read("legacy/dashboard/index.html").toString("utf8");
    const favicon = read("legacy/dashboard/favicon.svg").toString("utf8");

    expect(html).toContain('<link rel="icon" type="image/svg+xml" href="/favicon.svg" />');
    expect(favicon).toContain("<svg");
    expect(favicon).toContain("TinyTop");
  });
});
