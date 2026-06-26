import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard overview gauges", () => {
  test("renders load as a fourth overview gauge", () => {
    expect(html).toContain('id="load-gauge"');
    expect(html).toContain('id="load-value"');
    expect(html).toContain('id="load-capacity"');
    expect(html).toContain('id="load-spark"');
    expect(html).toContain('aria-label="Load pressure"');
    expect(html).toContain('aria-label="Load pressure history"');
  });

  test("updates load gauge from load average relative to CPU cores", () => {
    expect(app).toContain("loadGauge: document.querySelector(\"#load-gauge\")");
    expect(app).toContain("loadValue: document.querySelector(\"#load-value\")");
    expect(app).toContain("loadCapacity: document.querySelector(\"#load-capacity\")");
    expect(app).toContain("loadSpark: document.querySelector(\"#load-spark\")");
    expect(app).toContain("function loadPercent(snapshot)");
    expect(app).toContain("setGauge(elements.loadGauge, loadPressure)");
    expect(app).toContain("drawSparkline(elements.loadSpark, state.history.load");
  });
});
