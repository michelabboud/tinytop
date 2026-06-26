import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard settings", () => {
  test("renders browser and daemon settings groups", () => {
    expect(html).toContain('id="settings"');
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
    expect(app).toContain("fetchSettings");
    expect(app).toContain("saveDaemonSettings");
    expect(app).toContain('fetch("/api/settings"');
    expect(app).toContain('method: "PUT"');
    expect(app).toContain("restartPollingTimer");
  });
});
