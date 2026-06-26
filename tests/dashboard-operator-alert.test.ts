import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard operator alert strip", () => {
  test("renders a status strip for current operator state", () => {
    expect(html).toContain('id="operator-status"');
    expect(html).toContain('id="operator-state"');
    expect(html).toContain('id="operator-summary"');
    expect(html).toContain('id="operator-age"');
    expect(html).toContain('id="operator-offender"');
    expect(html).toContain('aria-live="polite"');
  });

  test("computes status from daemon thresholds and applies status attributes", () => {
    expect(app).toContain("operatorStatus: document.querySelector(\"#operator-status\")");
    expect(app).toContain("operatorState: document.querySelector(\"#operator-state\")");
    expect(app).toContain("operatorAge: document.querySelector(\"#operator-age\")");
    expect(app).toContain("function computeSnapshotStatus(snapshot");
    expect(app).toContain("function renderOperatorStatus(snapshot");
    expect(app).toContain("data-status");
    expect(app).toContain("loadCritical");
    expect(app).toContain("pressureCritical");
  });
});
