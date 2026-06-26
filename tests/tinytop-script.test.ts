import { describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
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
    expect(result.stdout).toContain("./tinytop rust collect");
    expect(result.stdout).toContain("./tinytop systemd install");
    expect(result.stdout).toContain("./tinytop systemd render [--rust|--bun]");
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
    expect(result.stdout).toContain("Rust/Cargo: missing");
    expect(result.stdout).toContain("rustup.rs");
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

  test("rust build reports missing Cargo with install guidance", async () => {
    const result = await runTinytop(["rust", "build"], {
      PATH: "/usr/bin:/bin",
    });

    expect(result.exitCode).toBe(1);
    expect(result.stderr).toContain("Rust/Cargo is required");
    expect(result.stderr).toContain("https://rustup.rs");
  });

  test("systemd render emits a single Rust collector/dashboard daemon by default", async () => {
    const result = await runTinytop(["systemd", "render"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("tinytop.service");
    expect(result.stdout).toContain("ExecStart=");
    expect(result.stdout).toContain("tinytop-agent serve");
    expect(result.stdout).not.toContain("--public-dir");
    expect(result.stdout).not.toContain("TINYTOP_PUBLIC_DIR");
    expect(result.stdout).not.toContain("tinytop-writer.service");
    expect(result.stdout).not.toContain("tinytop-dashboard.service");
    expect(result.stdout).not.toContain("run src/collector-daemon.ts");
    expect(result.stdout).not.toContain("run legacy/bun-collector.ts");
    expect(result.stdout).not.toContain("run src/server.ts");
    expect(result.stdout).not.toContain("User=");
  });

  test("legacy Bun collector lives under legacy and not src", () => {
    expect(existsSync(join(repoRoot, "legacy/bun-collector.ts"))).toBe(true);
    expect(existsSync(join(repoRoot, "src/collector-daemon.ts"))).toBe(false);
  });

  test("systemd render can still emit the legacy Bun collector", async () => {
    const result = await runTinytop(["systemd", "render", "--bun"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("tinytop-collector.service");
    expect(result.stdout).not.toContain("tinytop-writer.service");
    expect(result.stdout).toContain("run legacy/bun-collector.ts");
    expect(result.stdout).toContain("Description=TinyTop legacy Bun collector");
    expect(result.stdout).not.toContain("tinytop-agent serve");
  });

  test("systemd render prefers the current repo Rust binary over an installed local binary", async () => {
    const targetPath = join(repoRoot, "agent/target/release/tinytop-agent");
    if (!existsSync(targetPath)) return;

    const home = mkdtempSync(join(tmpdir(), "tinytop-home-"));
    const localBinDir = join(home, ".local/bin");
    mkdirSync(localBinDir, { recursive: true });
    const localPath = join(localBinDir, "tinytop-agent");
    writeFileSync(localPath, "#!/usr/bin/env bash\nprintf 'stale-local-agent\\n'\n");
    chmodSync(localPath, 0o755);

    const result = await runTinytop(["systemd", "render"], {
      HOME: home,
      PATH: "/usr/bin:/bin",
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain(`ExecStart=${targetPath} serve`);
    expect(result.stdout).not.toContain(localPath);
  });

  test("start auto-selects the Rust collector/dashboard when an agent binary is available", async () => {
    const dir = mkdtempSync(join(tmpdir(), "tinytop-runtime-"));
    const agentPath = join(dir, "tinytop-agent");
    writeFileSync(agentPath, "#!/usr/bin/env bash\nprintf 'agent:%s\\n' \"$*\"\n");
    chmodSync(agentPath, 0o755);

    const result = await runTinytop(["start"], {
      PATH: "/usr/bin:/bin",
      TINYTOP_AGENT_BIN: agentPath,
      TINYTOP_RUNTIME: "auto",
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Auto-selected Rust collector/dashboard daemon");
    expect(result.stdout).toContain("agent:serve");
  });

  test("start honors the legacy Bun runtime override", async () => {
    const dir = mkdtempSync(join(tmpdir(), "tinytop-runtime-"));
    const bunPath = join(dir, "bun");
    writeFileSync(bunPath, "#!/usr/bin/env bash\nprintf 'bun:%s\\n' \"$*\"\n");
    chmodSync(bunPath, 0o755);

    const result = await runTinytop(["start"], {
      PATH: `${dir}:/usr/bin:/bin`,
      TINYTOP_RUNTIME: "legacy",
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Auto-selected legacy Bun dashboard");
    expect(result.stdout).toContain("bun:run dev");
  });

  test("start honors bun as an alias for the legacy runtime override", async () => {
    const dir = mkdtempSync(join(tmpdir(), "tinytop-runtime-"));
    const bunPath = join(dir, "bun");
    writeFileSync(bunPath, "#!/usr/bin/env bash\nprintf 'bun:%s\\n' \"$*\"\n");
    chmodSync(bunPath, 0o755);

    const result = await runTinytop(["start"], {
      PATH: `${dir}:/usr/bin:/bin`,
      TINYTOP_RUNTIME: "bun",
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Auto-selected legacy Bun dashboard");
    expect(result.stdout).toContain("bun:run dev");
  });

  test("status reports the version endpoint when a daemon is running", async () => {
    const dir = mkdtempSync(join(tmpdir(), "tinytop-runtime-"));
    const curlPath = join(dir, "curl");
    writeFileSync(
      curlPath,
      [
        "#!/usr/bin/env bash",
        "case \"$*\" in",
        "  */api/version*) printf '%s\\n' '{\"version\":\"9.9.9\",\"runtime\":\"rust\",\"component\":\"collector-dashboard-daemon\",\"dashboard\":\"embedded\"}' ;;",
        "  *) exit 22 ;;",
        "esac",
        "",
      ].join("\n"),
    );
    chmodSync(curlPath, 0o755);

    const result = await runTinytop(["status"], {
      PATH: `${dir}:/usr/bin:/bin`,
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Running daemon: rust collector-dashboard-daemon v9.9.9 (embedded dashboard)");
  });

  test("stop does not kill unrelated installed tinytop-agent processes", async () => {
    const dir = mkdtempSync(join(tmpdir(), "tinytop-runtime-"));
    const pgrepPath = join(dir, "pgrep");
    writeFileSync(
      pgrepPath,
      [
        "#!/usr/bin/env bash",
        "printf '%s\\n' '999999 /home/demo/.local/bin/tinytop-agent serve'",
        "",
      ].join("\n"),
    );
    chmodSync(pgrepPath, 0o755);

    const result = await runTinytop(["stop"], {
      PATH: `${dir}:/usr/bin:/bin`,
      XDG_RUNTIME_DIR: mkdtempSync(join(tmpdir(), "tinytop-runtime-dir-")),
    });

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("no foreground TinyTop runtime was detected");
    expect(result.stdout).not.toContain("Stopping foreground TinyTop runtime");
  });
});
