import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard process and filesystem controls", () => {
  test("renders process search, density, sortable headers, and details dialog", () => {
    expect(html).toContain('id="process-search"');
    expect(html).toContain('id="process-density"');
    expect(html).toContain('data-process-sort="pid"');
    expect(html).toContain('data-process-sort="cpu"');
    expect(html).toContain('data-process-sort="memory"');
    expect(html).toContain('data-process-sort="rss"');
    expect(html).toContain('id="process-detail-dialog"');
    expect(html).toContain('id="process-detail-title"');
  });

  test("renders filesystem system-toggle and root filesystem card", () => {
    expect(html).toContain('id="filesystem-show-system"');
    expect(html).toContain('id="root-filesystem-card"');
    expect(html).toContain('id="root-filesystem-name"');
    expect(html).toContain('id="root-filesystem-usage"');
  });

  test("persists process and filesystem browser preferences locally", () => {
    expect(app).toContain("processSearch: document.querySelector(\"#process-search\")");
    expect(app).toContain("processDensity: document.querySelector(\"#process-density\")");
    expect(app).toContain("processSortButtons: Array.from(document.querySelectorAll(\"[data-process-sort]\"))");
    expect(app).toContain("filesystemShowSystem: document.querySelector(\"#filesystem-show-system\")");
    expect(app).toContain("function sortProcesses");
    expect(app).toContain("function filterFilesystems");
    expect(app).toContain("tinytop.processFilter");
    expect(app).toContain("tinytop.processSort");
    expect(app).toContain("tinytop.filesystemShowSystem");
  });
});
