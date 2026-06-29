import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");
const styles = readFileSync("legacy/dashboard/styles.css", "utf8");

describe("dashboard timestamp timeline", () => {
  test("offers explicit history range presets", () => {
    expect(html).toContain('data-history-window="live"');
    expect(html).toContain('data-history-window="15m"');
    expect(html).toContain('data-history-window="1h"');
    expect(html).toContain('data-history-window="6h"');
    expect(html).toContain('data-history-window="24h"');
    expect(html).toContain('data-history-window="7d"');
    expect(html).toContain('data-history-window="30d"');
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

  test("renders timeline rail and history coverage instead of a native scrubber", () => {
    expect(html).toContain('id="timeline-rail"');
    expect(html).toContain('aria-label="History timeline rail"');
    expect(html).toContain('id="history-coverage"');
    expect(html).toContain('id="history-oldest"');
    expect(html).toContain('id="history-newest"');
    expect(html).toContain('id="history-db-size"');
    expect(html).toContain('id="history-db-budget"');
    expect(html).toContain('id="history-budget-status"');
    expect(html).toContain('id="history-marker-list"');
    expect(html).not.toContain('id="history-scrubber"');
  });

  test("draws timeline rail, fetches rollup points, markers, coverage, and persists visible series locally", () => {
    expect(app).toContain("timelineRail: document.querySelector(\"#timeline-rail\")");
    expect(app).toContain("historyMarkerList: document.querySelector(\"#history-marker-list\")");
    expect(app).toContain("function drawTimelineRail");
    expect(app).toContain("function drawTimelineMarkers");
    expect(app).toContain("function timelineTimestampFromPointer");
    expect(app).toContain("function handleTimelinePointer");
    expect(app).toContain("function fetchHistoryCoverage");
    expect(app).toContain("function fetchHistoryPoints");
    expect(app).toContain("function fetchHistoryMarkers");
    expect(app).toContain("function renderHistoryCoverage");
    expect(app).toContain("function renderHistoryMarkers");
    expect(app).toContain('fetch(apiPath("/api/history/coverage")');
    expect(app).toContain('fetch(apiPath(`/api/history/points?${params}`)');
    expect(app).toContain('fetch(apiPath(`/api/history/markers?${params}`)');
    expect(app).toContain("tinytop.visibleSeries");
  });

  test("shows collector/dashboard version metadata", () => {
    expect(html).toContain('id="daemon-version"');
    expect(app).toContain('fetch(apiPath("/api/version")');
    expect(app).toContain("renderVersion");
  });

  test("keeps sidebar runtime identity compact", () => {
    expect(html).toContain('class="runtime-pill"');
    expect(html).toContain('id="runtime-summary"');
    expect(html).toContain('id="runtime-reason"');
    expect(app).toContain("formatRuntimeSummary");
    expect(app).toContain("elements.runtimeReason.title = reason");
    expect(styles).toContain(".runtime-pill");
    expect(styles).toContain(".runtime-reason");
    expect(styles).toContain("line-clamp");
  });

  test("warns when the browser platform and served daemon runtime differ", () => {
    expect(html).toContain('id="runtime-origin-notice"');
    expect(app).toContain("detectClientRuntimeKind");
    expect(app).toContain("renderRuntimeOriginNotice");
    expect(app).toContain("metadata.daemon?.os");
    expect(app).toContain("snapshot.identity.runtime.kind");
    expect(styles).toContain(".runtime-origin-notice");
  });

  test("supports iframe embed mode and theme query parameters", () => {
    expect(app).toContain("IS_EMBED_VIEW");
    expect(app).toContain("REQUESTED_THEME");
    expect(app).toContain('["dark", "midnight"]');
    expect(app).toContain('apiPath("/api/version")');
    expect(app).toContain('apiPath(`/api/history/points?${params}`)');
    expect(styles).toContain('body[data-embed="true"]');
    expect(styles).toContain('body[data-embed="true"] .rail');
    expect(styles).toContain('body[data-embed="true"] .control-deck');
  });
});
