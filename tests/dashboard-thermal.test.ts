import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import * as rules from "../agent/assets/dashboard/ladder-rules.js";

const sheepSensors = [
  { chip: "coretemp", kind: "temp", label: "Package id 0", value: 54, max: 105, crit: 105 },
  { chip: "coretemp", kind: "temp", label: "Core 0", value: 51, max: 105, crit: 105 },
  { chip: "coretemp", kind: "temp", label: "Core 1", value: 52, max: 105, crit: 105 },
  { chip: "coretemp", kind: "temp", label: "Core 2", value: 53, max: 105, crit: 105 },
  { chip: "coretemp", kind: "temp", label: "Core 3", value: 50, max: 105, crit: 105 },
];

function extractFunction(source: string, name: string): string {
  const start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`${name} not found in app.js`);
  const bodyStart = source.indexOf("{", start);
  let depth = 0;
  for (let index = bodyStart; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(start, index + 1);
  }
  throw new Error(`${name} body is incomplete`);
}

class FakeStyle {
  values = new Map<string, string>();

  setProperty(name: string, value: string): void {
    this.values.set(name, value);
  }
}

class FakeElement {
  readonly tagName: string;
  readonly style = new FakeStyle();
  readonly dataset: Record<string, string> = {};
  readonly attributes = new Map<string, string>();
  children: FakeElement[] = [];
  className = "";
  hidden = false;
  textContent = "";
  title = "";

  constructor(tagName = "div") {
    this.tagName = tagName;
  }

  append(...children: FakeElement[]): void {
    this.children.push(...children);
  }

  replaceChildren(...children: FakeElement[]): void {
    this.children = children;
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }
}

function descendants(node: FakeElement): FakeElement[] {
  return node.children.flatMap((child) => [child, ...descendants(child)]);
}

function withClass(node: FakeElement, className: string): FakeElement[] {
  return descendants(node).filter((child) => child.className.split(/\s+/u).includes(className));
}

function thermalRenderer() {
  const source = readFileSync("agent/assets/dashboard/app.js", "utf8");
  const panel = new FakeElement("section");
  const count = new FakeElement("span");
  const groups = new FakeElement("div");
  const elements = { thermalPanel: panel, thermalCount: count, thermalGroups: groups };
  const state = { daemonSettings: { enabledSections: { overview: true } } };
  const document = { createElement: (tagName: string) => new FakeElement(tagName) };
  const setText = (node: FakeElement | undefined, value: string) => {
    if (node) node.textContent = value;
  };
  const render = new Function(
    "elements",
    "state",
    "document",
    "setText",
    "formatSensorThreshold",
    "formatSensorValue",
    "groupSensorsByChip",
    "sensorBarPercent",
    "sensorSeverity",
    `${extractFunction(source, "renderThermals")}; return renderThermals;`,
  )(
    elements,
    state,
    document,
    setText,
    rules.formatSensorThreshold,
    rules.formatSensorValue,
    rules.groupSensorsByChip,
    rules.sensorBarPercent,
    rules.sensorSeverity,
  ) as (sensors?: unknown[]) => void;
  return { render, panel, count, groups };
}

describe("dashboard thermal formatting rules", () => {
  test("detects thermal capability only from a non-array settings block", () => {
    expect(rules.thermalCapabilityFrom(null)).toBe(false);
    expect(rules.thermalCapabilityFrom({})).toBe(false);
    expect(rules.thermalCapabilityFrom({ thermal: null })).toBe(false);
    expect(rules.thermalCapabilityFrom({ thermal: [] })).toBe(false);
    expect(rules.thermalCapabilityFrom({ thermal: {} })).toBe(true);
  });

  test("formats finite sensor values to one decimal and unknown values as an em dash", () => {
    expect(rules.formatSensorValue(54)).toBe("54.0 °C");
    expect(rules.formatSensorValue(null)).toBe("—");
    expect(rules.formatSensorValue(undefined)).toBe("—");
    expect(rules.formatSensorValue(Number.NaN)).toBe("—");
  });

  test("formats only present integer thresholds", () => {
    expect(rules.formatSensorThreshold(undefined, undefined)).toBe("");
    expect(rules.formatSensorThreshold(105, undefined)).toBe("max 105 °C");
    expect(rules.formatSensorThreshold(undefined, 105)).toBe("crit 105 °C");
    expect(rules.formatSensorThreshold(105, 105)).toBe("max 105 °C · crit 105 °C");
    expect(rules.formatSensorThreshold(91, 105)).toBe("max 91 °C · crit 105 °C");
  });

  test("uses crit then max as the bar ceiling and never invents an absent ceiling", () => {
    expect(rules.sensorBarPercent(54, undefined, undefined)).toBeNull();
    expect(rules.sensorBarPercent(54, 105, undefined)).toBeCloseTo(51.428571, 5);
    expect(rules.sensorBarPercent(54, 91, 105)).toBeCloseTo(51.428571, 5);
    expect(rules.sensorBarPercent(110, 91, 105)).toBe(100);
    expect(rules.sensorBarPercent(-2, 91, 105)).toBe(0);
  });

  test("derives critical, warning, and normal severity only from present thresholds", () => {
    expect(rules.sensorSeverity(54, undefined, undefined)).toBe("normal");
    expect(rules.sensorSeverity(106, 105, 105)).toBe("critical");
    expect(rules.sensorSeverity(95, 91, 105)).toBe("warn");
    expect(rules.sensorSeverity(90, 91, 105)).toBe("normal");
  });

  test("returns no chip groups for a missing or empty sensor list", () => {
    expect(rules.groupSensorsByChip(undefined)).toEqual([]);
    expect(rules.groupSensorsByChip([])).toEqual([]);
  });

  test("groups sheep's five coretemp readings in their original order", () => {
    expect(rules.groupSensorsByChip(sheepSensors)).toEqual([
      { chip: "coretemp", readings: sheepSensors },
    ]);
  });

  test("parses comma and newline separated chip names without hiding duplicates", () => {
    expect(rules.parseThermalExtraChips("coretemp, k10temp\napplesmc\ncoretemp")).toEqual([
      "coretemp",
      "k10temp",
      "applesmc",
      "coretemp",
    ]);
    expect(rules.formatThermalExtraChips(["coretemp", "k10temp"])).toBe("coretemp\nk10temp");
  });
});

describe("dashboard thermal settings rules", () => {
  test.each(["amdgpu", "i915", "nvme"])("rejects reserved non-CPU chip %s inline", (chip) => {
    expect(rules.validateThermalSettings({ enabled: true, extraChips: [chip] })).toEqual([
      rules.THERMAL_RESERVED_CHIP_ERROR,
    ]);
  });

  test("still accepts an ordinary explicitly configured CPU chip", () => {
    expect(rules.validateThermalSettings({ enabled: true, extraChips: ["cpu_thermal"] })).toEqual([]);
  });

  test("rejects a thermal chip name with a bad character", () => {
    expect(rules.validateThermalSettings({ enabled: true, extraChips: ["CoreTemp"] })).toEqual([
      "thermal.extraChips entries must match ^[a-z0-9_]{1,32}$",
    ]);
  });

  test("rejects a seventeenth thermal chip name", () => {
    const extraChips = Array.from({ length: 17 }, (_, index) => `chip_${index}`);
    expect(rules.validateThermalSettings({ enabled: true, extraChips })).toEqual([
      "thermal.extraChips must hold at most 16 entries",
    ]);
  });

  test("rejects duplicate thermal chip names", () => {
    expect(rules.validateThermalSettings({ enabled: true, extraChips: ["coretemp", "coretemp"] })).toEqual([
      "thermal.extraChips must not contain duplicates",
    ]);
  });

  test("omits unsupported thermal settings and sends only the supported block fields", () => {
    const settings = {
      defaultHistoryWindow: "live",
      thermal: { enabled: true, extraChips: ["applesmc"], ignored: "not-on-wire" },
    };
    expect(rules.settingsPutPayload(settings, false, false, false)).toEqual({
      defaultHistoryWindow: "live",
    });
    expect(rules.settingsPutPayload(settings, false, false, true)).toEqual({
      defaultHistoryWindow: "live",
      thermal: { enabled: true, extraChips: ["applesmc"] },
    });
  });

  test("keeps the existing two-argument ladder payload byte-identical", () => {
    const settings = {
      defaultHistoryWindow: "90d",
      retentionLadder: { l1: { keepDays: 3 }, l2: { keepDays: 30 } },
      otel: { enabled: false },
    };
    expect(JSON.stringify(rules.settingsPutPayload(settings, true))).toBe(JSON.stringify(settings));
  });

  test("keeps the existing three-argument OTel payload byte-identical", () => {
    const settings = {
      defaultHistoryWindow: "live",
      retentionLadder: { l1: { keepDays: 3 } },
      otel: { enabled: true },
    };
    expect(JSON.stringify(rules.settingsPutPayload(settings, false, false))).toBe(
      JSON.stringify({ defaultHistoryWindow: "live" }),
    );
  });
});

describe("dashboard thermal DOM contracts", () => {
  const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
  const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
  const styles = readFileSync("agent/assets/dashboard/styles.css", "utf8");

  test("places the hidden thermal panel after GPU and wires settings plus coverage", () => {
    expect(html.indexOf('id="gpu-panel"')).toBeLessThan(html.indexOf('id="thermal-panel"'));
    expect(html.indexOf('id="thermal-panel"')).toBeLessThan(html.indexOf('id="history"'));
    expect(html).toContain('class="panel thermal-panel" id="thermal-panel" data-section="overview" aria-label="Thermals" hidden');
    expect(html).toContain('id="thermal-settings-group" hidden');
    expect(html).toContain('id="daemon-thermal-enabled"');
    expect(html).toContain('id="daemon-thermal-extra-chips"');
    expect(html).toContain('id="thermal-status"');
    expect(app).toContain("renderThermals(snapshot.sensors)");
    expect(app).toContain("renderThermalCoverage(coverage?.thermal)");
    expect(styles).toContain(".thermal-panel");
    expect(styles).toContain("var(--amber)");
    expect(styles).toContain("var(--red)");
  });

  test("keeps the thermal settings group hidden without server capability", () => {
    const group = new FakeElement("fieldset");
    const elements = {
      historyLadderSettingsGroup: new FakeElement(),
      historyLadderUnavailable: new FakeElement(),
      exportSettingsButton: new FakeElement(),
      importSettingsButton: new FakeElement(),
      daemonRetentionHours: new FakeElement(),
      daemonRollupRetentionDays: new FakeElement(),
      daemonRetentionHoursDerived: new FakeElement(),
      daemonRollupRetentionDaysDerived: new FakeElement(),
      otelSettingsGroup: new FakeElement(),
      thermalSettingsGroup: group,
      advancedDocumentSettingsGroup: new FakeElement(),
      advancedSettingsUnavailable: new FakeElement(),
    };
    const state = {
      retentionLadderAvailable: false,
      otelAvailable: false,
      thermalAvailable: rules.thermalCapabilityFrom({ defaultHistoryWindow: "live" }),
      historyCoverage: null,
    };
    const sync = new Function(
      "elements",
      "state",
      "setHidden",
      "renderTierCoverage",
      "syncSettingsTabAvailability",
      `${extractFunction(app, "syncRetentionLadderAvailability")}; return syncRetentionLadderAvailability;`,
    )(
      elements,
      state,
      (node: FakeElement, hidden: boolean) => { node.hidden = hidden; },
      () => {},
      () => {},
    ) as () => void;
    sync();
    expect(group.hidden).toBe(true);
  });

  test("hides the thermal panel for absent and empty sensors and shows real readings", () => {
    const { render, panel, count, groups } = thermalRenderer();
    render(undefined);
    expect(panel.hidden).toBe(true);
    render([]);
    expect(panel.hidden).toBe(true);
    render(sheepSensors);
    expect(panel.hidden).toBe(false);
    expect(count.textContent).toBe("5 sensors");
    expect(withClass(groups, "thermal-reading")).toHaveLength(5);
    expect(withClass(groups, "thermal-threshold")[0]?.textContent).toBe("max 105 °C · crit 105 °C");
    expect(withClass(groups, "thermal-bar")).toHaveLength(5);
  });

  test("renders a threshold-free row with no threshold text and no bar element", () => {
    const { render, groups } = thermalRenderer();
    render([{ chip: "coretemp", kind: "temp", label: "Package id 0", value: 54 }]);
    const rows = withClass(groups, "thermal-reading");
    expect(rows).toHaveLength(1);
    expect(rows[0]?.attributes.get("aria-label")).toBe("coretemp Package id 0: 54.0 °C; state normal");
    expect(withClass(rows[0]!, "thermal-threshold")[0]?.textContent).toBe("");
    expect(withClass(rows[0]!, "thermal-bar")).toHaveLength(0);
  });

  test("shows thermal coverage only when enabled and never includes a sensor value", () => {
    const status = new FakeElement("div");
    const render = new Function(
      "elements",
      "setHidden",
      "formatCount",
      "formatCoverageTime",
      `${extractFunction(app, "renderThermalCoverage")}; return renderThermalCoverage;`,
    )(
      { historyThermalStatus: status },
      (node: FakeElement, hidden: boolean) => { node.hidden = hidden; },
      rules.formatCount,
      (value: number | undefined) => value === undefined ? "-" : `time-${value}`,
    ) as (thermal?: Record<string, unknown>) => void;

    render(undefined);
    expect(status.hidden).toBe(true);
    render({ enabled: false, sensorCount: 5 });
    expect(status.hidden).toBe(true);
    render({ enabled: true, sensorCount: 5, oldestCapturedAtMs: 100, newestCapturedAtMs: 200 });
    expect(status.hidden).toBe(false);
    expect(status.textContent).toBe("Thermals — 5 sensors · time-100 → time-200");
    expect(status.textContent).not.toContain("54");
  });
});
