import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import * as rules from "../agent/assets/dashboard/ladder-rules.js";

const trashcanAdapter = {
  id: "pci-0000:02:00.0",
  vendor: "amd",
  name: "0x1002:0x6810",
  driver: "amdgpu",
  memoryUsedBytes: 6_000_640,
  memoryTotalBytes: 2_147_483_648,
  temperatureC: 44,
};

describe("dashboard GPU formatting rules", () => {
  test("formats finite GPU percentages and renders unknown values as an em dash", () => {
    expect(rules.formatGpuPercent(12.5)).toBe("12.5%");
    expect(rules.formatGpuPercent(undefined)).toBe("—");
  });

  test("formats complete, used-only, and unavailable GPU memory", () => {
    expect(rules.formatGpuMemory(6_000_640, 2_147_483_648)).toBe("5.7 MiB / 2.0 GiB");
    expect(rules.formatGpuMemory(6_000_640, undefined)).toBe("5.7 MiB");
    expect(rules.formatGpuMemory(undefined, 2_147_483_648)).toBe("—");
  });

  test("rounds finite GPU temperatures and omits unavailable temperatures", () => {
    expect(rules.formatGpuTemperature(44)).toBe("44 °C");
    expect(rules.formatGpuTemperature(null)).toBe("");
  });

  test("describes trashcan's measured adapter without inventing busy data", () => {
    expect(rules.describeGpuAdapter(trashcanAdapter)).toEqual({
      name: "0x1002:0x6810",
      meta: "amd · amdgpu",
      busy: "—",
      memory: "5.7 MiB / 2.0 GiB",
      temperature: "44 °C",
    });
  });

  test("falls back to an adapter id when its name is empty", () => {
    expect(rules.describeGpuAdapter({ id: "card0", vendor: "intel", name: "", driver: "i915" })).toEqual({
      name: "card0",
      meta: "intel · i915",
      busy: "—",
      memory: "—",
      temperature: "",
    });
  });

  test("shows the process GPU column only when a finite value exists", () => {
    expect(rules.gpuColumnVisible([])).toBe(false);
    expect(rules.gpuColumnVisible([{}, { gpuPercent: undefined }])).toBe(false);
    expect(rules.gpuColumnVisible([{ gpuPercent: 0 }])).toBe(true);
  });

  test("maps missing GPU percentages below every finite sort value", () => {
    expect(rules.gpuPercentSortValue({ gpuPercent: 12.5 })).toBe(12.5);
    expect(rules.gpuPercentSortValue({})).toBe(-1);
  });
});

describe("dashboard GPU source contracts", () => {
  const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
  const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
  const styles = readFileSync("agent/assets/dashboard/styles.css", "utf8");

  test("contains the GPU panel and process GPU sort markup", () => {
    expect(html).toContain('id="gpu-panel"');
    expect(html).toContain('id="gpu-adapters"');
    expect(html).toContain('id="gpu-count"');
    expect(html).toContain('data-process-sort="gpu"');
    expect(html).toContain('class="gpu-cell"');
  });

  test("wires GPU rendering into every selected snapshot", () => {
    expect(app).toContain("function renderGpus");
    expect(app).toContain('gpuPanel: document.querySelector("#gpu-panel")');
    expect(app).toContain('const PROCESS_SORT_KEYS = new Set(["pid", "cpu", "memory", "rss", "gpu"]);');
    expect(app).toContain("renderGpus(snapshot.gpus)");
  });

  test("styles the GPU panel and conditionally visible process column", () => {
    expect(styles).toContain(".gpu-panel");
    expect(styles).toContain('.process-panel table:not([data-has-gpu="true"]) .gpu-cell');
  });
});
