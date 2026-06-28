import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

function readPowerShellScript(): string {
  expect(existsSync("tinytop.ps1")).toBe(true);
  return readFileSync("tinytop.ps1", "utf8");
}

function readCmdWrapper(): string {
  expect(existsSync("tinytop.cmd")).toBe(true);
  return readFileSync("tinytop.cmd", "utf8");
}

describe("Windows PowerShell command center", () => {
  test("ships a native Windows command center", () => {
    const script = readPowerShellScript();

    expect(script).toContain("TinyTop");
    expect(script).toContain("function Invoke-TinyTopRustBuild");
    expect(script).toContain("function Install-TinyTopRustBinary");
    expect(script).toContain("function Start-TinyTop");
    expect(script).toContain("function Stop-TinyTop");
  });

  test("uses Windows-local install, state, and log paths", () => {
    const script = readPowerShellScript();

    expect(script).toContain("$env:LOCALAPPDATA");
    expect(script).toContain("TinyTop");
    expect(script).toContain("tinytop-agent.exe");
    expect(script).toContain("tinytop.log");
    expect(script).toContain("tinytop.pid");
    expect(script).toContain("history.sqlite");
  });

  test("defaults Windows to its own dashboard port to avoid WSL loopback collisions", () => {
    const script = readPowerShellScript();

    expect(script).toContain("$WslDashboardPort = 4274");
    expect(script).toContain("else { 4275 }");
    expect(script).toContain("Detected another TinyTop daemon");
    expect(script).toContain("http://$DefaultHost`:$DefaultPort");
  });

  test("builds the Windows collector feature explicitly", () => {
    const script = readPowerShellScript();

    expect(script).toContain("--no-default-features");
    expect(script).toContain("--features");
    expect(script).toContain("windows-collector");
  });

  test("has explicit Windows service commands with elevation guidance", () => {
    const script = readPowerShellScript();

    expect(script).toContain("function Install-TinyTopService");
    expect(script).toContain("function Test-TinyTopAdmin");
    expect(script).toContain("function Confirm-TinyTopServiceElevation");
    expect(script).toContain("[Environment]::UserInteractive");
    expect(script).toContain("Read-Host");
    expect(script).toContain("Confirm-TinyTopServiceElevation -Action \"install\"");
    expect(script).toContain("Confirm-TinyTopServiceElevation -Action \"uninstall\"");
    expect(script).toContain("Confirm-TinyTopServiceElevation -Action \"start\"");
    expect(script).toContain("Confirm-TinyTopServiceElevation -Action \"stop\"");
    expect(script).toContain("Confirm-TinyTopServiceElevation -Action \"restart\"");
    expect(script).toContain("New-Service");
    expect(script).toContain("Start-Service");
    expect(script).toContain("Stop-Service");
    expect(script).toContain("Run PowerShell as Administrator");
  });

  test("keeps service subcommands as an array under strict mode", () => {
    const script = readPowerShellScript();

    expect(script).toContain("$Rest = if ($args.Count -gt 1) { @($args[1..($args.Count - 1)]) } else { @() }");
  });

  test("ships an execution-policy-safe cmd wrapper", () => {
    const wrapper = readCmdWrapper();

    expect(wrapper).toContain("powershell.exe");
    expect(wrapper).toContain("-ExecutionPolicy Bypass");
    expect(wrapper).toContain("tinytop.ps1");
  });

  test("does not mention systemd as the Windows service mechanism", () => {
    const script = readPowerShellScript();

    expect(script).not.toContain("systemd");
  });
});
