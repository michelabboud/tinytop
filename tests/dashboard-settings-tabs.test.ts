import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  advancedDocumentApplyAllowed,
  availableSettingsTabs,
  disabledMetricsFromSelection,
  groupMetricRegistry,
  moveSettingsTab,
  resolveSettingsTab,
  settingsPutPayload,
} from "../agent/assets/dashboard/ladder-rules.js";

const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
const app = readFileSync("agent/assets/dashboard/app.js", "utf8");

function extractFunction(source: string, name: string): string {
  let start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`${name} not found`);
  if (source.slice(start - 6, start) === "async ") start -= 6;
  const signatureEnd = source.indexOf(") {", start);
  const bodyStart = signatureEnd >= 0 ? signatureEnd + 2 : source.indexOf("{", start);
  let depth = 0;
  for (let index = bodyStart; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(start, index + 1);
  }
  throw new Error(`${name} is incomplete`);
}

const registry = [
  {
    name: "system.cpu.utilization",
    unit: "1",
    family: "CPU",
    description: "CPU utilization",
    semanticConvention: true,
    disabled: true,
  },
  {
    name: "system.cpu.load_average.1m",
    unit: "1",
    family: "CPU",
    description: "One-minute load average",
    semanticConvention: true,
    disabled: false,
  },
  {
    name: "system.memory.usage",
    unit: "By",
    family: "Memory",
    description: "Memory in use",
    semanticConvention: true,
    disabled: false,
  },
];

describe("tabbed settings shell", () => {
  test("renders one accessible tablist and five permanent labelled panels", () => {
    expect(html.match(/role="tablist"/gu)).toHaveLength(1);
    for (const id of ["general", "history", "metrics", "thermals", "advanced"]) {
      expect(html).toContain(`id="settings-tab-${id}"`);
      expect(html).toContain(`aria-controls="settings-panel-${id}"`);
      expect(html).toContain(`id="settings-panel-${id}"`);
      expect(html).toContain(`aria-labelledby="settings-tab-${id}"`);
    }
    expect(html.match(/role="tabpanel"/gu)).toHaveLength(5);
  });

  test("tab keyboard movement wraps and Home or End jumps to the boundary", () => {
    const tabs = ["general", "history", "metrics", "thermals", "advanced"];
    expect(moveSettingsTab("general", "ArrowLeft", tabs)).toBe("advanced");
    expect(moveSettingsTab("advanced", "ArrowRight", tabs)).toBe("general");
    expect(moveSettingsTab("metrics", "Home", tabs)).toBe("general");
    expect(moveSettingsTab("metrics", "End", tabs)).toBe("advanced");
  });

  test("tablist sets are exact for all capability combinations", () => {
    expect(availableSettingsTabs(false, false)).toEqual(["general", "history", "advanced"]);
    expect(availableSettingsTabs(true, false)).toEqual(["general", "history", "metrics", "advanced"]);
    expect(availableSettingsTabs(false, true)).toEqual(["general", "history", "thermals", "advanced"]);
    expect(availableSettingsTabs(true, true)).toEqual(["general", "history", "metrics", "thermals", "advanced"]);
  });

  test("remembered hidden thermals falls back to General", () => {
    const available = availableSettingsTabs(false, false);
    expect(resolveSettingsTab("thermals", available)).toBe("general");
    expect(available).not.toContain("thermals");
  });

  test("every capability combination resolves to a selected tab that exists", () => {
    for (const metricsAvailable of [false, true]) {
      for (const thermalAvailable of [false, true]) {
        const available = availableSettingsTabs(metricsAvailable, thermalAvailable);
        expect(available).toContain(resolveSettingsTab("metrics", available));
      }
    }
  });

  test("switching every tab is a view change and never marks settings dirty", () => {
    const tabs = ["general", "history", "metrics", "thermals", "advanced"].map((name) => ({
      dataset: { settingsTab: name },
      attributes: new Map<string, string>(),
      tabIndex: -1,
      setAttribute(key: string, value: string) { this.attributes.set(key, value); },
      focus() {},
    }));
    const panels = tabs.map((tab) => ({ dataset: { settingsPanel: tab.dataset.settingsTab }, hidden: false }));
    const state = { metricsAvailable: true, thermalAvailable: true, activeSettingsTab: "general", settingsDirty: false };
    const dirtyIndicator = { hidden: true };
    const select = new Function(
      "state",
      "elements",
      "availableSettingsTabs",
      "resolveSettingsTab",
      "setHidden",
      "persistSettingsTab",
      `${extractFunction(app, "selectSettingsTab")}; return selectSettingsTab;`,
    )(
      state,
      { settingsTabs: tabs, settingsPanels: panels },
      availableSettingsTabs,
      resolveSettingsTab,
      (node: { hidden: boolean }, hidden: boolean) => { node.hidden = hidden; },
      () => {},
    ) as (tab: string) => string;
    for (const tab of ["general", "history", "metrics", "thermals", "advanced"]) {
      expect(select(tab)).toBe(tab);
      expect(state.settingsDirty).toBe(false);
      expect(dirtyIndicator.hidden).toBe(true);
    }
  });

  test("Metrics selection leaves History and Thermals controls permanently in the DOM", () => {
    const metricsPanelStart = html.indexOf('id="settings-panel-metrics"');
    expect(metricsPanelStart).toBeGreaterThan(0);
    expect(html).toContain('id="settings-panel-history"');
    expect(html).toContain('id="daemon-l1-keep-days"');
    expect(html).toContain('id="settings-panel-thermals"');
    expect(html).toContain('id="daemon-thermal-extra-chips"');
    expect(app).not.toContain(".removeChild(elements.settingsPanel");
    expect(app).not.toContain(".remove() // settings panel");
  });

  test("collectDaemonSettingsFromForm keeps History and Thermals values while Metrics is selected", () => {
    const control = (value: string, checked = false) => ({ value, checked });
    const elements: Record<string, unknown> = {
      daemonL1KeepDays: control("17"),
      daemonL2KeepDays: control("45"),
      daemonL3Enabled: control("", true),
      daemonL3KeepDays: control("120"),
      daemonL4Enabled: control("", true),
      daemonL4KeepDays: control("900"),
      daemonL4Forever: control("", false),
      daemonDetailIntervalSec: control("75"),
      daemonProcessFastKeepHours: control("36"),
      daemonArchiveQueryable: control("", false),
      daemonArchiveCold: control("", false),
      daemonArchiveColdAfterMonths: control("12"),
      daemonArchiveDirectory: control(""),
      daemonDiskCheckIntervalMinutes: control("60"),
      daemonDiskCheckMinFreeGib: control("5"),
      daemonThermalEnabled: control("", true),
      daemonThermalExtraChips: control("cpu_thermal"),
    };
    const daemonSettings = {
      retentionHours: 72,
      rollupRetentionDays: 30,
      retentionLadder: {},
      otel: { disabledMetrics: [] },
      thermal: { enabled: false, extraChips: [] },
      thresholds: {},
      enabledSections: {},
    };
    const state = {
      activeSettingsTab: "metrics",
      retentionLadderAvailable: true,
      otelAvailable: false,
      thermalAvailable: true,
      daemonSettings,
      otelResourceAttributeErrors: [],
    };
    const collect = new Function(
      "state",
      "elements",
      "cloneSettings",
      "numberControlValue",
      "numericControlValue",
      "parseResourceAttributes",
      "parseThermalExtraChips",
      "disabledMetricsFromSelection",
      "DEFAULT_POLL_MS",
      `${extractFunction(app, "collectDaemonSettingsFromForm")}; return collectDaemonSettingsFromForm;`,
    )(
      state,
      elements,
      (value: unknown) => structuredClone(value),
      (node: { value?: string } | undefined, fallback: number) => Number.isFinite(Number(node?.value)) ? Math.round(Number(node?.value)) : fallback,
      (node: { value?: string } | undefined, fallback: number) => Number.isFinite(Number(node?.value)) ? Number(node?.value) : fallback,
      () => ({ attributes: {}, errors: [] }),
      (text: string) => text.split(/[\s,]+/u).filter(Boolean),
      disabledMetricsFromSelection,
      1_500,
    ) as () => Record<string, any>;
    const collected = collect();
    expect(collected.retentionLadder.l1.keepDays).toBe(17);
    expect(collected.retentionLadder.l2.keepDays).toBe(45);
    expect(collected.thermal).toEqual({ enabled: true, extraChips: ["cpu_thermal"] });
  });

  test("selected tab persistence reads and writes localStorage defensively", () => {
    expect(app).toContain('settingsTab: "tinytop.settingsTab"');
    expect(app).toMatch(/function readStoredSettingsTab\([\s\S]*?try \{/u);
    expect(app).toMatch(/function persistSettingsTab\([\s\S]*?try \{/u);
  });
});

describe("Metrics picker rules", () => {
  function metricFetcher(fetchImpl: () => Promise<Response>) {
    const state = {
      metricsAvailable: true,
      metricRegistry: registry,
      unknownDisabledMetrics: ["old.unknown"],
      pendingSettingsTab: null,
      activeSettingsTab: "metrics",
    };
    let availabilitySyncs = 0;
    const fetchMetricRegistry = new Function(
      "state",
      "elements",
      "fetch",
      "apiPath",
      "renderMetricRegistry",
      "syncSettingsTabAvailability",
      `${extractFunction(app, "fetchMetricRegistry")}; return fetchMetricRegistry;`,
    )(
      state,
      { metricsSettingsGroups: { replaceChildren() {} } },
      fetchImpl,
      (path: string) => path,
      () => {},
      () => { availabilitySyncs += 1; },
    ) as () => Promise<void>;
    return { fetchMetricRegistry, state, availabilitySyncs: () => availabilitySyncs };
  }

  test("a non-OK metrics response hides the Metrics tab capability", async () => {
    const fixture = metricFetcher(async () => new Response("missing", { status: 404 }));
    await fixture.fetchMetricRegistry();
    expect(fixture.state.metricsAvailable).toBe(false);
    expect(fixture.state.metricRegistry).toEqual([]);
    expect(fixture.availabilitySyncs()).toBe(1);
  });

  test("a metrics network error hides the Metrics tab capability", async () => {
    const fixture = metricFetcher(async () => { throw new Error("offline"); });
    await fixture.fetchMetricRegistry();
    expect(fixture.state.metricsAvailable).toBe(false);
    expect(fixture.state.unknownDisabledMetrics).toEqual([]);
    expect(fixture.availabilitySyncs()).toBe(1);
  });

  test("groups registry entries by family without changing registry order", () => {
    expect(groupMetricRegistry(registry)).toEqual([
      { family: "CPU", metrics: [registry[0], registry[1]] },
      { family: "Memory", metrics: [registry[2]] },
    ]);
  });

  test("checking every metric produces an empty disabled set", () => {
    expect(disabledMetricsFromSelection(registry, registry.map((metric) => metric.name), [])).toEqual([]);
  });

  test("unchecking only system.cpu.utilization stores exactly that disabled name", () => {
    expect(disabledMetricsFromSelection(
      registry,
      ["system.cpu.load_average.1m", "system.memory.usage"],
      [],
    )).toEqual(["system.cpu.utilization"]);
  });

  test("unknown_disabled_metrics_survive_a_save_untouched", () => {
    const disabledMetrics = disabledMetricsFromSelection(
      registry,
      registry.map((metric) => metric.name),
      ["system.future.metric"],
    );
    expect(settingsPutPayload({ otel: { disabledMetrics } }, false, true)).toEqual({
      otel: { disabledMetrics: ["system.future.metric"] },
    });
  });

  test("metrics UI is route-driven and includes family select-all and inert unknown output", () => {
    expect(app).toContain('fetch(apiPath("/api/otel/metrics")');
    expect(html).toContain('id="metrics-settings-groups"');
    expect(html).toContain('id="metrics-settings-unknown"');
    expect(html).toContain("unknown on this version (inert)");
    expect(app).toContain("metricFamilyToggle");
  });
});

describe("Advanced raw document", () => {
  test("Apply starts disabled and the editor exposes Validate then Apply", () => {
    expect(html).toContain('id="advanced-settings-document"');
    expect(html).toContain('id="validate-advanced-settings-button"');
    expect(html).toContain('id="apply-advanced-settings-button" type="button" disabled');
  });

  test("only a successful validation of the current exact text authorizes Apply", () => {
    expect(advancedDocumentApplyAllowed("same", "same", true)).toBe(true);
    expect(advancedDocumentApplyAllowed("changed", "same", true)).toBe(false);
    expect(advancedDocumentApplyAllowed("same", "same", false)).toBe(false);
    expect(advancedDocumentApplyAllowed("", null, true)).toBe(false);
  });

  test("malformed JSON is converted to a validation error instead of escaping", () => {
    expect(app).toContain("Advanced document is not JSON:");
    expect(app).toMatch(/async function validateAdvancedSettingsDocument[\s\S]*?JSON\.parse[\s\S]*?catch/u);
  });

  test("server failure stays disabled while server success renders changed keys and enables Apply", async () => {
    const elements = {
      advancedSettingsDocument: { value: "{\"tinytopConfigVersion\":1}" },
      applyAdvancedSettingsButton: { disabled: true },
    };
    const state = { advancedValidatedText: null as string | null, advancedValidationSucceeded: false };
    const rendered: Array<{ messages: string[]; outcome?: boolean }> = [];
    let plan: Record<string, unknown> = { valid: false, errors: ["server says no"] };
    const invalidate = () => {
      state.advancedValidatedText = null;
      state.advancedValidationSucceeded = false;
      elements.applyAdvancedSettingsButton.disabled = true;
    };
    const validate = new Function(
      "elements",
      "state",
      "invalidateAdvancedDocumentValidation",
      "renderSettingsValidation",
      "renderSettingsStatus",
      "previewSettingsImport",
      "isValidImportPlan",
      "advancedDocumentApplyAllowed",
      `${extractFunction(app, "validateAdvancedSettingsDocument")}; return validateAdvancedSettingsDocument;`,
    )(
      elements,
      state,
      invalidate,
      (messages: string[], options: { outcome?: boolean } = {}) => rendered.push({ messages, ...options }),
      () => {},
      async () => plan,
      () => true,
      advancedDocumentApplyAllowed,
    ) as () => Promise<void>;

    elements.advancedSettingsDocument.value = "{";
    await validate();
    expect(rendered.at(-1)?.messages[0]).toStartWith("Advanced document is not JSON:");
    expect(elements.applyAdvancedSettingsButton.disabled).toBe(true);

    elements.advancedSettingsDocument.value = "{\"tinytopConfigVersion\":1}";
    await validate();
    expect(rendered.at(-1)?.messages).toEqual(["server says no"]);
    expect(elements.applyAdvancedSettingsButton.disabled).toBe(true);

    plan = { valid: true, changedKeys: ["otel"] };
    await validate();
    expect(rendered.at(-1)).toEqual({ messages: ["Would apply: otel."], outcome: true });
    expect(elements.applyAdvancedSettingsButton.disabled).toBe(false);

    elements.advancedSettingsDocument.value += " ";
    invalidate();
    expect(elements.applyAdvancedSettingsButton.disabled).toBe(true);
  });

  test("input invalidates stale validation immediately", () => {
    expect(app).toMatch(/advancedSettingsDocument\?\.addEventListener\("input"[\s\S]*?invalidateAdvancedDocumentValidation/u);
  });
});

describe("save error body", () => {
  test("a settings save uses the server message before the HTTP fallback", async () => {
    expect(app).toMatch(/await responseErrorMessage\(response, `Settings save failed with HTTP \$\{response\.status\}`\)/u);
    const responseErrorMessage = new Function(
      `${extractFunction(app, "responseErrorMessage")}; return responseErrorMessage;`,
    )() as (response: Response, fallback: string) => Promise<string>;
    expect(await responseErrorMessage(
      Response.json({ error: "disabledMetrics contains a duplicate" }, { status: 400 }),
      "Settings save failed with HTTP 400",
    )).toBe("disabledMetrics contains a duplicate");
  });
});
