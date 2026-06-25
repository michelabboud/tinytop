import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { extname, join } from "node:path";

const WEB_UI_EXTENSIONS = new Set([".css", ".html", ".js"]);
const DASHBOARD_DIR = "legacy/dashboard";

function collectWebUiFiles(root: string): string[] {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    if (entry.isDirectory()) return entry.name === "vendor" ? [] : collectWebUiFiles(path);
    return WEB_UI_EXTENSIONS.has(extname(entry.name)) ? [path] : [];
  });
}

const webUiFiles = collectWebUiFiles(DASHBOARD_DIR);

function read(path: string): string {
  return readFileSync(path, "utf8");
}

describe("web UI dialogs", () => {
  test("does not use browser-native alert, confirm, or prompt APIs", () => {
    const nativeDialogApi = /(?:\bwindow\.|\bglobalThis\.)?(alert|confirm|prompt)\s*\(/;

    for (const file of webUiFiles) {
      expect(read(file), `${file} should not invoke native browser dialogs`).not.toMatch(nativeDialogApi);
    }
  });

  test("uses status-message naming instead of alert naming for inline errors", () => {
    expect(read(`${DASHBOARD_DIR}/index.html`)).not.toContain('id="alert"');
    expect(read(`${DASHBOARD_DIR}/index.html`)).not.toContain('class="alert"');
    expect(read(`${DASHBOARD_DIR}/styles.css`)).not.toContain(".alert");
    expect(read(`${DASHBOARD_DIR}/app.js`)).not.toContain("elements.alert");
  });

  test("provides an accessible in-app confirmation dialog", () => {
    const html = read(`${DASHBOARD_DIR}/index.html`);
    const app = read(`${DASHBOARD_DIR}/app.js`);

    expect(html).toContain('id="confirmation-dialog"');
    expect(html).toContain('aria-labelledby="confirmation-title"');
    expect(html).toContain('aria-describedby="confirmation-message"');
    expect(html).toContain('id="confirmation-confirm"');
    expect(html).toContain('id="confirmation-cancel"');
    expect(app).toContain("requestConfirmation");
    expect(app).toContain("confirmationDialog");
  });
});
