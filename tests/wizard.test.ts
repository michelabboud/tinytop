import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { buildSetupSummary, runWizard, type WizardCommand } from "../src/wizard/index";

function tempRepo(withNodeModules: boolean): string {
  const dir = mkdtempSync(join(tmpdir(), "tinytop-wizard-"));
  if (withNodeModules) mkdirSync(join(dir, "node_modules"), { recursive: true });
  return dir;
}

describe("TinyTop setup wizard", () => {
  test("builds a setup summary with ports, DB path, and runtime mode", () => {
    const summary = buildSetupSummary({
      dashboardUrl: "http://127.0.0.1:4274",
      collectorUrl: "http://127.0.0.1:4276",
      dbPath: "/home/demo/.local/share/tinytop/history.sqlite",
      mode: "systemd",
      runChecks: true,
      startServices: true,
      collectorRuntime: "rust",
      rustBinarySource: "release",
    });

    expect(summary).toContain("Runtime mode: systemd");
    expect(summary).toContain("Collector runtime: Rust");
    expect(summary).toContain("Dashboard: http://127.0.0.1:4274");
    expect(summary).toContain("Collector API: http://127.0.0.1:4276");
    expect(summary).not.toContain("Writer:");
    expect(summary).toContain("SQLite: /home/demo/.local/share/tinytop/history.sqlite");
    expect(summary).toContain("Verification: bun run check");
    expect(summary).toContain("Rust collector binary: GitHub release binary");
    expect(summary).not.toContain("Rust agent:");
  });

  test("noninteractive setup skips dependency install when node_modules exists", async () => {
    const repo = tempRepo(true);
    const commands: WizardCommand[] = [];
    const output: string[] = [];

    try {
      const result = await runWizard({
        cwd: repo,
        args: ["--non-interactive", "--skip-checks"],
        stdout: (line) => output.push(line),
        stderr: (line) => output.push(line),
        runCommand: async (command) => {
          commands.push(command);
          return { ok: true, code: 0 };
        },
        env: {
          HOME: "/home/demo",
        },
      });

      expect(result.exitCode).toBe(0);
      expect(commands).toEqual([]);
      expect(output.join("\n")).toContain("TinyTop setup wizard");
      expect(output.join("\n")).toContain("Collector runtime: Rust");
      expect(output.join("\n")).toContain("./tinytop rust serve");
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });

  test("noninteractive setup installs dependencies and runs checks when requested", async () => {
    const repo = tempRepo(false);
    const commands: WizardCommand[] = [];

    try {
      const result = await runWizard({
        cwd: repo,
        args: ["--non-interactive"],
        stdout: () => {},
        stderr: () => {},
        runCommand: async (command) => {
          commands.push(command);
          return { ok: true, code: 0 };
        },
        env: {
          HOME: "/home/demo",
        },
      });

      expect(result.exitCode).toBe(0);
      expect(commands).toEqual([
        ["bun", "install"],
        ["bun", "run", "check"],
      ]);
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });

  test("systemd mode installs and optionally starts user services", async () => {
    const repo = tempRepo(true);
    const commands: WizardCommand[] = [];

    try {
      const result = await runWizard({
        cwd: repo,
        args: ["--non-interactive", "--skip-checks", "--mode", "systemd", "--collector", "rust", "--rust-binary-source", "compile", "--start-services"],
        stdout: () => {},
        stderr: () => {},
        runCommand: async (command) => {
          commands.push(command);
          return { ok: true, code: 0 };
        },
        env: {
          HOME: "/home/demo",
        },
      });

      expect(result.exitCode).toBe(0);
      expect(commands).toEqual([
        ["./tinytop", "rust", "build"],
        ["./tinytop", "systemd", "install", "--rust"],
        ["./tinytop", "systemd", "start"],
      ]);
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });

  test("systemd mode can request a GitHub release binary before installing services", async () => {
    const repo = tempRepo(true);
    const commands: WizardCommand[] = [];
    const output: string[] = [];

    try {
      const result = await runWizard({
        cwd: repo,
        args: ["--non-interactive", "--skip-checks", "--mode", "systemd", "--collector", "rust", "--rust-binary-source", "release"],
        stdout: (line) => output.push(line),
        stderr: (line) => output.push(line),
        runCommand: async (command) => {
          commands.push(command);
          return { ok: true, code: 0 };
        },
        env: {
          HOME: "/home/demo",
        },
      });

      expect(result.exitCode).toBe(0);
      expect(commands).toEqual([
        ["./tinytop", "rust", "install-binary"],
        ["./tinytop", "systemd", "install", "--rust"],
      ]);
      expect(output.join("\n")).toContain("Rust collector binary: GitHub release binary");
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });

  test("systemd mode can install the legacy Bun collector instead of the Rust collector", async () => {
    const repo = tempRepo(true);
    const commands: WizardCommand[] = [];
    const output: string[] = [];

    try {
      const result = await runWizard({
        cwd: repo,
        args: ["--non-interactive", "--skip-checks", "--mode", "systemd", "--collector", "bun", "--start-services"],
        stdout: (line) => output.push(line),
        stderr: (line) => output.push(line),
        runCommand: async (command) => {
          commands.push(command);
          return { ok: true, code: 0 };
        },
        env: {
          HOME: "/home/demo",
        },
      });

      expect(result.exitCode).toBe(0);
      expect(commands).toEqual([
        ["./tinytop", "systemd", "install", "--bun"],
        ["./tinytop", "systemd", "start"],
      ]);
      expect(output.join("\n")).toContain("Collector runtime: Legacy Bun");
      expect(output.join("\n")).not.toContain("Rust collector binary: GitHub release binary");
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });
});
