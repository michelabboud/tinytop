import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard settings", () => {
  test("renders settings as a dialog instead of an inline dashboard section", () => {
    expect(html).toContain('id="settings-dialog"');
    expect(html).toContain('aria-labelledby="settings-title"');
    expect(html).toContain('id="settings-open-button"');
    expect(html).toContain('id="close-settings-button"');
    expect(html).not.toContain('<section class="panel settings-panel" id="settings"');
    expect(html).not.toContain('href="#settings"');
  });

  test("renders browser and daemon settings groups", () => {
    expect(html).toContain("This Browser");
    expect(html).toContain("This Daemon");
    expect(html).toContain('id="browser-theme-setting"');
    expect(html).toContain('id="browser-graph-setting"');
    expect(html).toContain('id="browser-history-window-setting"');
    expect(html).toContain('id="daemon-poll-interval"');
    expect(html).toContain('id="daemon-retention-hours"');
    expect(html).toContain('id="save-settings-button"');
  });

  test("keeps browser preferences local and daemon settings API-backed", () => {
    expect(app).toContain("tinytop.theme");
    expect(app).toContain("tinytop.visibleSeries");
    expect(app).toContain("tinytop.processFilter");
    expect(app).toContain("tinytop.processDensity");
    expect(app).toContain("tinytop.filesystemShowSystem");
    expect(app).toContain("tinytop.lastSection");
    expect(app).toContain("fetchSettings");
    expect(app).toContain("saveDaemonSettings");
    expect(app).toContain("openSettingsDialog");
    expect(app).toContain("closeSettingsDialog");
    expect(app).toContain('fetch("/api/settings"');
    expect(app).toContain('method: "PUT"');
    expect(app).toContain("restartPollingTimer");
  });

  test("renders daemon section toggles and expanded threshold settings", () => {
    for (const id of [
      "daemon-cpu-critical",
      "daemon-memory-critical",
      "daemon-disk-critical",
      "daemon-load-warn",
      "daemon-load-critical",
      "daemon-pressure-warn",
      "daemon-pressure-critical",
      "daemon-section-overview",
      "daemon-section-history",
      "daemon-section-filesystem",
      "daemon-section-pressure",
      "daemon-section-processes",
    ]) {
      expect(html).toContain(`id="${id}"`);
    }

    expect(app).toContain("function normalizeThresholds");
    expect(app).toContain("function applyEnabledSections");
    expect(app).toContain("function metricStatus");
    expect(app).toContain("enabledSections");
  });
});
