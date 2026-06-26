import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard timestamp timeline", () => {
  test("offers explicit history range presets", () => {
    expect(html).toContain('data-history-window="live"');
    expect(html).toContain('data-history-window="15m"');
    expect(html).toContain('data-history-window="1h"');
    expect(html).toContain('data-history-window="6h"');
    expect(html).toContain('data-history-window="24h"');
  });

  test("tracks selection by timestamp instead of sample index", () => {
    expect(app).toContain("selectedAtMs");
    expect(app).toContain("selectHistoryTimestamp");
    expect(app).not.toContain("selectedSampleIndex");
  });

  test("fetches history with timestamp windows and paginates large ranges", () => {
    expect(app).toContain("since_ms");
    expect(app).toContain("until_ms");
    expect(app).toContain("fetchHistoryPage");
    expect(app).toContain("MAX_HISTORY_PAGE_SIZE");
    expect(app).not.toContain("window_seconds=${HISTORY_WINDOW_SECONDS}");
  });

  test("persists only this browser's selected history window locally", () => {
    expect(app).toContain("tinytop.historyWindow");
    expect(app).toContain("readStoredValue(STORAGE_KEYS.historyWindow");
    expect(app).toContain("storeValue(STORAGE_KEYS.historyWindow");
  });

  test("shows collector/dashboard version metadata", () => {
    expect(html).toContain('id="daemon-version"');
    expect(app).toContain('fetch("/api/version"');
    expect(app).toContain("renderVersion");
  });
});
