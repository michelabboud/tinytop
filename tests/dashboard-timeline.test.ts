import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  liveSampleDrivesTiles,
  liveSampleEntersHistory,
} from "../agent/assets/dashboard/ladder-rules.js";

function extractFunction(source: string, name: string): string {
  let start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`${name} not found`);
  if (source.slice(start - 6, start) === "async ") start -= 6;
  const bodyStart = source.indexOf("{", start);
  let depth = 0;
  for (let index = bodyStart; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(start, index + 1);
  }
  throw new Error(`${name} is incomplete`);
}

const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
const styles = readFileSync("agent/assets/dashboard/styles.css", "utf8");

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

  test("fetches every non-raw preset as one auto page and keeps response metadata", () => {
    expect(app).toContain('fetchHistoryPoints({ sinceMs, untilMs, limit, source: "auto" })');
    expect(app).toContain("resolutionMs");
    expect(app).toContain("available: body.available !== false");
    expect(app).toContain('sample.source !== "raw"');
    expect(app).not.toContain('sample.source === "rollup"');
  });

  test("renders the not-yet-queryable archive response as an empty state", () => {
    expect(app).toContain('historyResponse.available === false && historyResponse.source === "archive"');
    expect(app).toContain("Archive not available until 0.4.0");
  });

  test("falls back without persisting when coverage disables the selected preset", () => {
    expect(app).toContain("fallbackWindowKey");
    expect(app).toContain("setHistoryWindow(fallbackWindow, { persist: false })");
  });

  test("marks missing coverage as a Bun runtime and explains disabled presets", () => {
    expect(app).toContain("renderHistoryCoverage({ unavailable: true })");
    expect(app).toContain("History presets beyond 1h need the Rust daemon");
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

describe("a historical window is not pushed sideways by live samples", () => {
  test("only the live window charts the freshly polled sample", () => {
    expect(liveSampleEntersHistory("live")).toBe(true);
    for (const key of ["15m", "1h", "6h", "24h", "7d", "30d", "90d", "1y", "all"]) {
      expect(liveSampleEntersHistory(key)).toBe(false);
    }
  });

  test("the live sample still drives the tiles on a historical window until one is selected", () => {
    // Break caught: silencing the live push must not freeze the gauges.
    expect(liveSampleDrivesTiles("30d", null)).toBe(true);
    expect(liveSampleDrivesTiles("30d", 1_788_244_538_225)).toBe(false);
    // On the live window the pushed sample already renders the tiles.
    expect(liveSampleDrivesTiles("live", null)).toBe(false);
  });

  test("renderSnapshot routes the poll through those rules instead of always pushing", () => {
    const renderSnapshot = extractFunction(app, "renderSnapshot");
    expect(renderSnapshot).toContain("liveSampleEntersHistory(state.historyWindowKey)");
    expect(renderSnapshot).toContain("liveSampleDrivesTiles(state.historyWindowKey, state.selectedAtMs)");
    // The push must sit INSIDE the live-window guard. An unconditional push
    // evicted the chosen window one point per tick once it hit the render cap.
    expect(renderSnapshot).toMatch(/if \(liveSampleEntersHistory[^)]*\)\) pushHistory\(snapshot\);/u);
    // Break caught: the live tile render must come AFTER renderSelectedSample,
    // which otherwise re-renders the tiles from the window's last stored sample
    // and freezes the gauges on any raw-tier window.
    expect(renderSnapshot.indexOf("renderSnapshotDetails(snapshot)")).toBeGreaterThan(
      renderSnapshot.indexOf("renderSelectedSample()"),
    );
  });
});
