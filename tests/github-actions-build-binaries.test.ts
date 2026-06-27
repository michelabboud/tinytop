import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

function readWorkflow(): string {
  const path = ".github/workflows/build-binaries.yml";
  expect(existsSync(path)).toBe(true);
  return readFileSync(path, "utf8");
}

describe("on-demand binary build workflow", () => {
  test("is manually dispatched with platform and release controls", () => {
    const workflow = readWorkflow();

    expect(workflow).toContain("workflow_dispatch:");
    expect(workflow).toContain("platform:");
    expect(workflow).toContain("type: choice");
    expect(workflow).toContain("- all");
    expect(workflow).toContain("- linux");
    expect(workflow).toContain("- windows");
    expect(workflow).toContain("- macos");
    expect(workflow).toContain("release_tag:");
    expect(workflow).toContain("upload_to_release:");
    expect(workflow).toContain("type: boolean");
  });

  test("builds the supported release asset matrix on native hosted runners", () => {
    const workflow = readWorkflow();

    expect(workflow).toContain("ubuntu-24.04");
    expect(workflow).toContain("windows-2025");
    expect(workflow).toContain("macos-15-intel");
    expect(workflow).toContain("macos-15");
    expect(workflow).toContain("tinytop-agent-linux-x86_64");
    expect(workflow).toContain("tinytop-agent-windows-x86_64.exe");
    expect(workflow).toContain("tinytop-agent-macos-x86_64");
    expect(workflow).toContain("tinytop-agent-macos-aarch64");
    expect(workflow).toContain("--no-default-features");
    expect(workflow).toContain("--features");
    expect(workflow).toContain("linux-collector");
    expect(workflow).toContain("windows-collector");
    expect(workflow).toContain("macos-collector");
  });

  test("uploads artifacts and can attach binaries to an existing release", () => {
    const workflow = readWorkflow();

    expect(workflow).toContain("actions/checkout@v7");
    expect(workflow).toContain("actions/upload-artifact@v7");
    expect(workflow).toContain("sha256sum");
    expect(workflow).toContain("shasum -a 256");
    expect(workflow).toContain("gh release upload");
    expect(workflow).toContain("--clobber");
    expect(workflow).toContain("contents: write");
  });
});
