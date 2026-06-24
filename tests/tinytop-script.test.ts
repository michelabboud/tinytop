import { describe, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { join } from "node:path";

const repoRoot = new URL("..", import.meta.url).pathname;
const scriptPath = join(repoRoot, "tinytop");
const bashPath = existsSync("/bin/bash") ? "/bin/bash" : "/usr/bin/bash";

type RunResult = {
  exitCode: number;
  stdout: string;
  stderr: string;
};

async function runTinytop(args: string[], env: Record<string, string | undefined> = {}): Promise<RunResult> {
  const proc = Bun.spawn([bashPath, scriptPath, ...args], {
    cwd: repoRoot,
    stdout: "pipe",
    stderr: "pipe",
    env: {
      ...process.env,
      NO_COLOR: "1",
      ...env,
    },
  });

  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  return { exitCode, stdout, stderr };
}

describe("tinytop command center", () => {
  test("help works before Bun-specific commands and documents operations", async () => {
    const result = await runTinytop(["help"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("TinyTop");
    expect(result.stdout).toContain("./tinytop setup");
    expect(result.stdout).toContain("./tinytop systemd install");
    expect(result.stdout).toContain("./tinytop db backup");
    expect(result.stdout).toContain("bun run setup");
  });

  test("install-bun --print-only prints the official command without running it", async () => {
    const result = await runTinytop(["install-bun", "--print-only"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout.trim()).toBe("curl -fsSL https://bun.com/install | bash");
    expect(result.stderr).toBe("");
  });

  test("doctor reports missing Bun without failing", async () => {
    const result = await runTinytop(["doctor"], {
      PATH: "/usr/bin:/bin",
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Bun: missing");
    expect(result.stdout).toContain("./tinytop install-bun --yes");
  });

  test("setup stops with clear Bun guidance when Bun is missing", async () => {
    const result = await runTinytop(["setup"], {
      PATH: "/usr/bin:/bin",
    });

    expect(result.exitCode).toBe(1);
    expect(result.stderr).toContain("Bun is required before the Bun setup wizard can run");
    expect(result.stderr).toContain("./tinytop install-bun --yes");
  });

  test("db reset refuses to run without --yes", async () => {
    const result = await runTinytop(["db", "reset"]);

    expect(result.exitCode).toBe(1);
    expect(result.stderr).toContain("Refusing to reset history without --yes");
  });

  test("systemd render emits split user services for writer and dashboard", async () => {
    const result = await runTinytop(["systemd", "render"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("tinytop-writer.service");
    expect(result.stdout).toContain("tinytop-dashboard.service");
    expect(result.stdout).toContain("ExecStart=");
    expect(result.stdout).toContain("run src/collector-daemon.ts");
    expect(result.stdout).toContain("run src/server.ts");
    expect(result.stdout).toContain("Requires=tinytop-writer.service");
    expect(result.stdout).toContain("Environment=TINYTOP_DISABLE_WRITER_SPAWN=1");
    expect(result.stdout).not.toContain("User=");
  });
});
