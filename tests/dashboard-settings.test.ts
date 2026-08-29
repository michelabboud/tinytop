import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  ladderCapabilityFrom,
  otelCapabilityFrom,
  settingsPutPayload,
} from "../agent/assets/dashboard/ladder-rules.js";

const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
const styles = readFileSync("agent/assets/dashboard/styles.css", "utf8");

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
    expect(html).toContain('id="daemon-db-budget-mib"');
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
    expect(app).toContain('fetch(apiPath("/api/settings")');
    expect(app).toContain('method: "PUT"');
    expect(app).toContain("restartPollingTimer");
  });

  test("renders settings validation, reset, presets, dirty guard, and effective readout", () => {
    expect(html).toContain('id="settings-validation-summary"');
    expect(html).toContain('id="settings-dirty-indicator"');
    expect(html).toContain('id="threshold-preset"');
    expect(html).toContain('id="reset-settings-button"');
    expect(html).toContain('id="restore-default-settings-button"');
    expect(html).toContain('id="effective-settings-readout"');
    expect(app).toContain("function validateDaemonSettings");
    expect(app).toContain("function markSettingsDirty");
    expect(app).toContain("function resetSettingsForm");
    expect(app).toContain("function restoreDefaultSettings");
    expect(app).toContain("function applyThresholdPreset");
    expect(app).toContain("function renderEffectiveSettings");
    expect(app).toContain("function confirmSettingsDismissIfDirty");
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

  test("renders the complete history ladder settings and derived legacy mirrors", () => {
    expect(html).toContain("History ladder");
    for (const id of [
      "daemon-l1-keep-days",
      "daemon-l2-keep-days",
      "daemon-l3-enabled",
      "daemon-l3-keep-days",
      "daemon-l4-enabled",
      "daemon-l4-keep-days",
      "daemon-l4-forever",
      "daemon-snapshot-json-keep-minutes",
      "daemon-detail-interval-sec",
      "daemon-process-fast-keep-hours",
      "daemon-archive-queryable",
      "daemon-archive-cold",
      "daemon-archive-cold-after-months",
      "daemon-archive-directory",
      "daemon-disk-check-interval-minutes",
      "daemon-disk-check-min-free-gib",
    ]) {
      expect(html).toContain(`id="${id}"`);
    }
    expect(html).toContain('id="daemon-retention-hours" type="number" readonly');
    expect(html).toContain('id="daemon-rollup-retention-days" type="number" readonly');
    expect(html).toContain("derived from L1/L2");
    expect(app).toContain(
      'daemonProcessFastKeepHours: document.querySelector("#daemon-process-fast-keep-hours")',
    );
    expect(app).toContain("processFastKeepHours: numberControlValue(");
  });

  test("omits retentionLadder from a Bun runtime PUT payload", () => {
    const runtimeSettings = {
      defaultHistoryWindow: "90d",
      retentionHours: 72,
      rollupRetentionDays: 30,
    };
    const normalizedInternalSettings = {
      ...runtimeSettings,
      retentionLadder: {
        l1: { keepDays: 3 },
        l2: { keepDays: 30 },
      },
    };

    expect(ladderCapabilityFrom(null)).toBe(false);
    expect(ladderCapabilityFrom(runtimeSettings)).toBe(false);
    expect(ladderCapabilityFrom(normalizedInternalSettings)).toBe(true);
    expect(settingsPutPayload(normalizedInternalSettings, false)).toEqual(runtimeSettings);
  });

  test("declares the Rust-only ladder replacement line", () => {
    expect(html).toContain('id="history-ladder-unavailable"');
    expect(html).toContain("History ladder — Rust daemon only");
    expect(app).toContain("ladderCapabilityFrom");
    expect(app).toContain("settingsPutPayload");
  });

  test("renders the optional OpenTelemetry settings group and all controls", () => {
    expect(html).toContain('id="otel-settings-group"');
    expect(html).toContain('class="settings-group otel-settings-group"');
    expect(html).toContain('id="daemon-otel-enabled"');
    expect(html).toContain('id="daemon-otel-endpoint"');
    expect(html).toContain('id="daemon-otel-interval-sec"');
    expect(html).toContain('id="daemon-otel-headers-env-var"');
    expect(html).toContain('id="daemon-otel-service-name"');
    expect(html).toContain('id="daemon-otel-resource-attributes"');
    expect(html).toContain('id="history-otel-status"');
    expect(html).toContain("Headers (e.g. authorization) are read from the environment variable named here — never stored in settings.");
    expect(app).toContain("otelCapabilityFrom");
    expect(app).toContain("state.otelAvailable");
  });

  test("renders the new history presets and ladder coverage surfaces", () => {
    for (const window of ["90d", "1y", "all"]) {
      expect(html).toContain(`data-history-window="${window}"`);
      expect(html).toContain(`<option value="${window}">`);
    }
    expect(html).toContain('id="history-ladder-coverage"');
    expect(html).toContain('id="history-disk-pressure"');
    expect(html).toContain('id="history-archive-status"');
    expect(app).toContain("historyWindowFor");
    expect(app).toContain("validateRetentionLadder");
  });

  test("provides Rust-only settings transfer controls without client-side approximations", () => {
    expect(html).toContain('id="export-settings-button"');
    expect(html).toContain('id="import-settings-button"');
    expect(html).toContain('id="import-settings-file"');
    expect(app).not.toContain("approx");
  });

  test("omits OpenTelemetry from a runtime PUT payload when unsupported", () => {
    const settings = { defaultHistoryWindow: "live", otel: { enabled: true } };
    expect(otelCapabilityFrom(null)).toBe(false);
    expect(otelCapabilityFrom({ defaultHistoryWindow: "live" })).toBe(false);
    expect(otelCapabilityFrom(settings)).toBe(true);
    expect(settingsPutPayload(settings, false, false)).toEqual({ defaultHistoryWindow: "live" });
    expect(settingsPutPayload(settings, false)).toEqual({ defaultHistoryWindow: "live" });
  });

  test("keeps native select dropdown options readable in every theme", () => {
    for (const selector of [
      ".settings-group select option",
      ".process-controls select option",
      'body[data-theme="matrix"] .settings-group select option',
      'body[data-theme="aurora"] .settings-group select option',
      'body[data-theme="solar"] .settings-group select option',
      'body[data-theme="ember"] .settings-group select option',
    ]) {
      expect(styles).toContain(selector);
    }

    expect(styles).toContain("background: #1c1110;");
    expect(styles).toContain("color: #fff7ed;");
    expect(styles).toContain("background: #ffffff;");
    expect(styles).toContain("color: #0f172a;");
  });
});
