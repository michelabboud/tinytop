import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { createFetchHandler } from "../src/server";
import {
  DEFAULT_DASHBOARD_SETTINGS,
  DEFAULT_TOP_PROCESS_COUNT,
  MAX_TOP_PROCESS_COUNT,
  MIN_TOP_PROCESS_COUNT,
  normalizeDashboardSettings,
} from "../src/settings";
import type { SystemSnapshot } from "../src/collector";

// `topProcessCount` is how many processes every sample keeps, in the live list
// and in the two process history tables. Its default and bounds are restated
// in five places that cannot import one another: the Rust store, the Rust
// collector, the Bun settings module, the dashboard script and the dialog's
// `<input>`. The default moved 8 -> 12 on 2026-10-09; a copy left behind would
// have the daemon, `collect --json`, the dev server and the form disagree
// without any error. This file is the one place that reads all five.

const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
const store = readFileSync("agent/crates/tinytop-store/src/lib.rs", "utf8");
const collectors = readFileSync("agent/crates/tinytop-collectors/src/lib.rs", "utf8");

function rustConst(source: string, name: string): number {
  const match = new RegExp(`^pub const ${name}: (?:i64|usize) = (\\d+);$`, "mu").exec(source);
  if (!match) throw new Error(`${name} not found`);
  return Number(match[1]);
}

function scriptConst(name: string): number {
  const match = new RegExp(`^const ${name} = (\\d+);$`, "mu").exec(app);
  if (!match) throw new Error(`${name} not found in app.js`);
  return Number(match[1]);
}

describe("the default and the bounds of topProcessCount agree everywhere", () => {
  test("the default is twelve in both runtimes and in the dashboard", () => {
    expect(DEFAULT_TOP_PROCESS_COUNT).toBe(12);
    expect(DEFAULT_DASHBOARD_SETTINGS.topProcessCount).toBe(12);
    expect(rustConst(store, "DEFAULT_TOP_PROCESS_COUNT")).toBe(12);
    expect(rustConst(collectors, "DEFAULT_TOP_PROCESS_COUNT")).toBe(12);
    expect(scriptConst("DEFAULT_TOP_PROCESS_COUNT")).toBe(12);
  });

  test("the bounds are 1 and 50 in the store, the Bun module, the script and the input", () => {
    const input = /<input id="daemon-top-process-count"[^>]*>/u.exec(html)?.[0] ?? "";
    const bounds = {
      store: [rustConst(store, "MIN_TOP_PROCESS_COUNT"), rustConst(store, "MAX_TOP_PROCESS_COUNT")],
      bun: [MIN_TOP_PROCESS_COUNT, MAX_TOP_PROCESS_COUNT],
      script: [scriptConst("MIN_TOP_PROCESS_COUNT"), scriptConst("MAX_TOP_PROCESS_COUNT")],
      input: [Number(/ min="(\d+)"/u.exec(input)?.[1]), Number(/ max="(\d+)"/u.exec(input)?.[1])],
    };
    expect(bounds).toEqual({ store: [1, 50], bun: [1, 50], script: [1, 50], input: [1, 50] });
  });

  test("no bare literal is left where the named value belongs", () => {
    // The three sites that used to carry `8`, `1, 50` and `8` in the script,
    // and the two in the Rust store. A reintroduced literal is how the copies
    // drift apart again.
    expect(app).toContain("topProcessCount: DEFAULT_TOP_PROCESS_COUNT,");
    expect(app).toContain("numberControlValue(elements.daemonTopProcessCount, DEFAULT_TOP_PROCESS_COUNT)");
    expect(app).toContain("settings.topProcessCount, MIN_TOP_PROCESS_COUNT, MAX_TOP_PROCESS_COUNT)");
    expect(store).toContain("top_process_count: DEFAULT_TOP_PROCESS_COUNT,");
    expect(store).not.toMatch(/validate_range\(\s*"topProcessCount",\s*self\.top_process_count,\s*\d/u);
    expect(collectors).toContain("top_process_count: DEFAULT_TOP_PROCESS_COUNT,");
  });
});

describe("the Bun settings boundary", () => {
  const complete = () => structuredClone(DEFAULT_DASHBOARD_SETTINGS) as Record<string, unknown>;

  test("a configured count inside the range is kept", () => {
    for (const count of [MIN_TOP_PROCESS_COUNT, 8, 12, 20, MAX_TOP_PROCESS_COUNT]) {
      expect(normalizeDashboardSettings({ ...complete(), topProcessCount: count }).topProcessCount).toBe(count);
    }
  });

  test("a document written before the field mattered loads with the default", () => {
    const older = complete();
    delete older.topProcessCount;
    expect(normalizeDashboardSettings(older).topProcessCount).toBe(12);
  });

  test("a count outside the range is refused with the shared range message", () => {
    for (const count of [0, -1, MAX_TOP_PROCESS_COUNT + 1, 1_000]) {
      expect(() => normalizeDashboardSettings({ ...complete(), topProcessCount: count })).toThrow(
        "topProcessCount must be between 1 and 50",
      );
    }
  });

  test("PUT /api/settings answers 400 with { error } and keeps the stored count", async () => {
    const handler = createFetchHandler({
      publicDir: "/missing",
      collect: async () => ({ snapshot: {} as SystemSnapshot, currentProcStatText: "cpu 1 0 1 8" }),
    });
    const put = (topProcessCount: unknown) =>
      handler(
        new Request("http://127.0.0.1:4274/api/settings", {
          method: "PUT",
          body: JSON.stringify({ ...complete(), topProcessCount }),
        }),
      );

    const accepted = await put(20);
    expect(accepted.status).toBe(200);
    expect((await accepted.json()).topProcessCount).toBe(20);

    for (const rejected of [0, 51]) {
      const response = await put(rejected);
      expect(response.status).toBe(400);
      expect(await response.json()).toEqual({ error: "topProcessCount must be between 1 and 50" });
    }

    const after = await (await handler(new Request("http://127.0.0.1:4274/api/settings"))).json();
    expect(after.topProcessCount).toBe(20);
  });
});
