import { existsSync } from "node:fs";
import { stdin as nodeStdin, stdout as nodeStdout } from "node:process";
import { createInterface } from "node:readline/promises";
import { resolveHistoryDbPath } from "../ops";

export type WizardMode = "foreground" | "split" | "systemd" | "skip";
export type RustBinarySource = "release" | "compile";
export type CollectorRuntime = "rust" | "bun";
export type WizardCommand = string[];

export type CommandResult = {
  ok: boolean;
  code: number;
};

export type WizardOptions = {
  cwd?: string;
  args?: string[];
  env?: Record<string, string | undefined>;
  stdout?: (line: string) => void;
  stderr?: (line: string) => void;
  runCommand?: (command: WizardCommand, options: { cwd: string; env: Record<string, string | undefined> }) => Promise<CommandResult>;
};

export type WizardResult = {
  exitCode: number;
};

export type SetupSummary = {
  dashboardUrl: string;
  collectorUrl: string;
  dbPath: string;
  mode: WizardMode;
  runChecks: boolean;
  startServices: boolean;
  collectorRuntime: CollectorRuntime;
  rustBinarySource: RustBinarySource;
};

type ParsedArgs = {
  nonInteractive: boolean;
  runChecks: boolean;
  mode: WizardMode;
  startServices: boolean;
  collectorRuntime: CollectorRuntime;
  rustBinarySource: RustBinarySource;
};

type VerificationPlan = {
  label: string;
  command: WizardCommand;
};

const MODES = new Set<WizardMode>(["foreground", "split", "systemd", "skip"]);
const RUST_BINARY_SOURCES = new Set<RustBinarySource>(["release", "compile"]);
const COLLECTOR_RUNTIMES = new Set<CollectorRuntime>(["rust", "bun"]);

function dashboardUrl(env: Record<string, string | undefined>): string {
  return `http://${env.HOST ?? "127.0.0.1"}:${env.PORT ?? "4274"}`;
}

function collectorUrl(env: Record<string, string | undefined>): string {
  return `http://${env.HISTORY_WRITER_HOST ?? "127.0.0.1"}:${env.HISTORY_WRITER_PORT ?? "4276"}`;
}

function parseArgs(args: string[]): ParsedArgs {
  const parsed: ParsedArgs = {
    nonInteractive: false,
    runChecks: true,
    mode: "foreground",
    startServices: false,
    collectorRuntime: "rust",
    rustBinarySource: "release",
  };

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--") continue;
    else if (arg === "--non-interactive") parsed.nonInteractive = true;
    else if (arg === "--skip-checks") parsed.runChecks = false;
    else if (arg === "--start-services") parsed.startServices = true;
    else if (arg === "--skip-start") parsed.startServices = false;
    else if (arg === "--collector") {
      const value = args[index + 1] as CollectorRuntime | undefined;
      if (!value || !COLLECTOR_RUNTIMES.has(value)) {
        throw new Error(`Invalid --collector value: ${value ?? "<missing>"}`);
      }
      parsed.collectorRuntime = value;
      index += 1;
    }
    else if (arg === "--rust-binary-source") {
      const value = args[index + 1] as RustBinarySource | undefined;
      if (!value || !RUST_BINARY_SOURCES.has(value)) {
        throw new Error(`Invalid --rust-binary-source value: ${value ?? "<missing>"}`);
      }
      parsed.rustBinarySource = value;
      index += 1;
    }
    else if (arg === "--mode") {
      const value = args[index + 1] as WizardMode | undefined;
      if (!value || !MODES.has(value)) {
        throw new Error(`Invalid --mode value: ${value ?? "<missing>"}`);
      }
      parsed.mode = value;
      index += 1;
    } else {
      throw new Error(`Unknown setup option: ${arg}`);
    }
  }

  return parsed;
}

function verificationPlanFor(summary: Pick<SetupSummary, "collectorRuntime" | "rustBinarySource">): VerificationPlan {
  if (summary.collectorRuntime === "bun") {
    return {
      label: "Bun collector checks",
      command: ["bun", "run", "check:bun"],
    };
  }

  if (summary.rustBinarySource === "compile") {
    return {
      label: "Rust collector checks",
      command: ["bun", "run", "check:rust"],
    };
  }

  return {
    label: "Rust release binary smoke check",
    command: ["./tinytop", "rust", "collect"],
  };
}

export function buildSetupSummary(summary: SetupSummary): string {
  const verification = verificationPlanFor(summary).label;
  const lines = [
    "TinyTop setup wizard",
    `Runtime mode: ${summary.mode}`,
    `Collector runtime: ${summary.collectorRuntime === "rust" ? "Rust" : "Legacy Bun"}`,
    `Dashboard: ${summary.dashboardUrl}`,
    `Collector API: ${summary.collectorUrl}`,
    `SQLite: ${summary.dbPath}`,
    `Verification: ${summary.runChecks ? verification : "skipped"}`,
    `Start services: ${summary.startServices ? "yes" : "no"}`,
  ];

  if (summary.collectorRuntime === "rust") {
    lines.splice(7, 0, `Rust collector binary: ${summary.rustBinarySource === "release" ? "GitHub release binary" : "local Cargo compile"}`);
  }

  return lines.join("\n");
}

async function defaultRunCommand(command: WizardCommand, options: { cwd: string; env: Record<string, string | undefined> }): Promise<CommandResult> {
  const proc = Bun.spawn(command, {
    cwd: options.cwd,
    env: options.env,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  const code = await proc.exited;
  return { ok: code === 0, code };
}

async function promptForInteractiveArgs(parsed: ParsedArgs, out: (line: string) => void): Promise<ParsedArgs> {
  const rl = createInterface({ input: nodeStdin, output: nodeStdout });
  try {
    out("Choose collector runtime: rust collector or bun collector");
    const collectorAnswer = (await rl.question(`Collector runtime [${parsed.collectorRuntime}]: `)).trim();
    if (collectorAnswer) {
      if (!COLLECTOR_RUNTIMES.has(collectorAnswer as CollectorRuntime)) throw new Error(`Invalid collector runtime: ${collectorAnswer}`);
      parsed.collectorRuntime = collectorAnswer as CollectorRuntime;
    }

    out("Choose runtime mode: foreground, split, systemd, skip");
    const modeAnswer = (await rl.question(`Runtime mode [${parsed.mode}]: `)).trim();
    if (modeAnswer) {
      if (!MODES.has(modeAnswer as WizardMode)) throw new Error(`Invalid runtime mode: ${modeAnswer}`);
      parsed.mode = modeAnswer as WizardMode;
    }

    const checksAnswer = (await rl.question(`Run runtime-specific verification? [Y/n]: `)).trim().toLowerCase();
    parsed.runChecks = checksAnswer === "" || checksAnswer === "y" || checksAnswer === "yes";

    if (parsed.mode === "systemd" && parsed.collectorRuntime === "rust") {
      out("Choose Rust collector binary install method: release or compile");
      const sourceAnswer = (await rl.question(`Rust collector binary [${parsed.rustBinarySource}]: `)).trim();
      if (sourceAnswer) {
        if (!RUST_BINARY_SOURCES.has(sourceAnswer as RustBinarySource)) throw new Error(`Invalid Rust collector binary install method: ${sourceAnswer}`);
        parsed.rustBinarySource = sourceAnswer as RustBinarySource;
      }

      const startAnswer = (await rl.question(`Start systemd services now? [y/N]: `)).trim().toLowerCase();
      parsed.startServices = startAnswer === "y" || startAnswer === "yes";
    } else if (parsed.mode === "systemd") {
      const startAnswer = (await rl.question(`Start systemd services now? [y/N]: `)).trim().toLowerCase();
      parsed.startServices = startAnswer === "y" || startAnswer === "yes";
    }

    return parsed;
  } finally {
    rl.close();
  }
}

async function runStep(
  label: string,
  command: WizardCommand,
  runCommand: NonNullable<WizardOptions["runCommand"]>,
  cwd: string,
  env: Record<string, string | undefined>,
  out: (line: string) => void,
  err: (line: string) => void,
): Promise<boolean> {
  out(`Running: ${command.join(" ")}`);
  const result = await runCommand(command, { cwd, env });
  if (!result.ok) {
    err(`${label} failed with exit ${result.code}.`);
    return false;
  }
  out(`${label} complete.`);
  return true;
}

export async function runWizard(options: WizardOptions = {}): Promise<WizardResult> {
  const cwd = options.cwd ?? process.cwd();
  const args = options.args ?? [];
  const env = { ...process.env, ...(options.env ?? {}) };
  const out = options.stdout ?? console.log;
  const err = options.stderr ?? console.error;
  const runCommand = options.runCommand ?? defaultRunCommand;

  let parsed: ParsedArgs;
  try {
    parsed = parseArgs(args);
  } catch (error) {
    err(error instanceof Error ? error.message : "Invalid setup options");
    return { exitCode: 1 };
  }

  if (!parsed.nonInteractive) {
    try {
      parsed = await promptForInteractiveArgs(parsed, out);
    } catch (error) {
      err(error instanceof Error ? error.message : "Interactive setup failed");
      return { exitCode: 1 };
    }
  }

  out(
    buildSetupSummary({
      dashboardUrl: dashboardUrl(env),
      collectorUrl: collectorUrl(env),
      dbPath: resolveHistoryDbPath({ env }),
      mode: parsed.mode,
      runChecks: parsed.runChecks,
      startServices: parsed.startServices,
      collectorRuntime: parsed.collectorRuntime,
      rustBinarySource: parsed.rustBinarySource,
    }),
  );

  if (!existsSync(`${cwd}/node_modules`)) {
    if (!(await runStep("Dependency install", ["bun", "install"], runCommand, cwd, env, out, err))) {
      return { exitCode: 1 };
    }
  } else {
    out("Dependencies: already installed.");
  }

  if (parsed.mode === "systemd") {
    if (parsed.collectorRuntime === "rust") {
      if (parsed.rustBinarySource === "release") {
        if (!(await runStep("Rust collector", ["./tinytop", "rust", "install-binary"], runCommand, cwd, env, out, err))) {
          return { exitCode: 1 };
        }
      }
    }
  }

  if (parsed.runChecks) {
    const verification = verificationPlanFor(parsed);
    if (!(await runStep("Verification", verification.command, runCommand, cwd, env, out, err))) {
      return { exitCode: 1 };
    }
  } else {
    out("Verification: skipped by request.");
  }

  if (parsed.mode === "systemd") {
    if (parsed.collectorRuntime === "rust") {
      if (parsed.rustBinarySource === "compile") {
        if (!(await runStep("Rust collector", ["./tinytop", "rust", "build"], runCommand, cwd, env, out, err))) {
          return { exitCode: 1 };
        }
      }
      if (!(await runStep("systemd install", ["./tinytop", "systemd", "install", "--rust"], runCommand, cwd, env, out, err))) {
        return { exitCode: 1 };
      }
    } else if (!(await runStep("Legacy Bun collector", ["./tinytop", "systemd", "install", "--bun"], runCommand, cwd, env, out, err))) {
      return { exitCode: 1 };
    }

    if (parsed.startServices) {
      if (!(await runStep("systemd start", ["./tinytop", "systemd", "start"], runCommand, cwd, env, out, err))) {
        return { exitCode: 1 };
      }
    } else {
      out("Start later with: ./tinytop systemd start");
    }
  } else if (parsed.mode === "split") {
    if (parsed.collectorRuntime === "rust") {
      out("Start Rust collector API with: ./tinytop rust serve-writer");
      out("Start dashboard with: TINYTOP_DISABLE_WRITER_SPAWN=1 bun run dev");
    } else {
      out("Start split legacy Bun collector mode with: ./tinytop start:split");
    }
  } else if (parsed.mode === "foreground") {
    if (parsed.collectorRuntime === "rust") {
      out("Start foreground Rust collector with: ./tinytop rust serve");
    } else {
      out("Start foreground legacy Bun collector mode with: ./tinytop start");
    }
  } else {
    out("Runtime start skipped.");
  }

  out(`Dashboard URL: ${dashboardUrl(env)}`);
  out("Setup complete.");
  return { exitCode: 0 };
}

if (process.argv[1]?.endsWith("src/wizard/index.ts")) {
  const result = await runWizard({ args: process.argv.slice(2) });
  process.exit(result.exitCode);
}
