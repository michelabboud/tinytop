# Windows Command Center And Critical Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a practical native Windows control layer for the Rust collector/dashboard and make Critical operator state visually unmistakable in the dashboard.

**Architecture:** Keep the Rust collector/dashboard daemon as the runtime. Add `tinytop.ps1` as the Windows sibling of the Bash `tinytop` command center, using PowerShell for bootstrap, foreground lifecycle, status, logs, and Windows service registration. Keep Linux/WSL as the verified reference runtime while making Windows source and release-binary flows explicit.

**Tech Stack:** PowerShell 5+/7, Rust/Cargo, existing `tinytop-agent`, Bash command center, static dashboard HTML/CSS/JS, Bun tests.

## Global Constraints

- Linux/WSL remains the reference runtime; Windows collector support is opt-in through `windows-collector`.
- No GitHub Actions minutes are required for this plan.
- `tinytop.ps1` must not require Bun.
- Windows service installation must be explicit and must fail with clear admin guidance when not elevated.
- The current GitHub release may not contain a Windows `.exe`; install flow must explain release-binary fallback to local compile.
- Dashboard critical/warning/stale states must not rely on color alone; visible text labels and state treatment remain required.
- Rust embedded dashboard assets and legacy Bun dashboard assets must remain byte-identical.

---

## Four-Step Plan

1. **PowerShell command center:** Add `tinytop.ps1` with `help`, `doctor`, `rust install-binary`, `rust build`, `start`, `stop`, `restart`, `status`, and `logs`.
2. **Windows build/install path:** Make Windows builds use `--no-default-features --features windows-collector`; keep release binary download paths predictable as `tinytop-agent-windows-x86_64.exe`.
3. **Windows service path:** Add explicit `service install|uninstall|start|stop|restart|status` commands in PowerShell using Windows Service Control Manager, requiring elevation only for install/uninstall.
4. **Critical status visibility:** Make Critical/Warning/Stale operator strip states visually stronger with full-strip treatment, state pill styling, and accessible text emphasis.

## Task 1: Regression Tests For Windows Installer Surface

**Files:**
- Create: `tests/tinytop-powershell.test.ts`
- Modify: `tests/tinytop-script.test.ts`

**Interfaces:**
- Produces a static contract for `tinytop.ps1` commands and Windows service behavior.
- Produces Bash wrapper coverage for target-specific Rust feature selection.

- [ ] **Step 1: Write failing PowerShell static tests**

Create `tests/tinytop-powershell.test.ts` with tests asserting:

```ts
import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

const script = readFileSync("tinytop.ps1", "utf8");

describe("Windows PowerShell command center", () => {
  test("ships a native Windows command center", () => {
    expect(existsSync("tinytop.ps1")).toBe(true);
    expect(script).toContain("TinyTop");
    expect(script).toContain("function Invoke-TinyTopRustBuild");
    expect(script).toContain("function Install-TinyTopRustBinary");
    expect(script).toContain("function Start-TinyTop");
    expect(script).toContain("function Stop-TinyTop");
  });

  test("uses Windows-local install, state, and log paths", () => {
    expect(script).toContain("$env:LOCALAPPDATA");
    expect(script).toContain("TinyTop");
    expect(script).toContain("tinytop-agent.exe");
    expect(script).toContain("tinytop.log");
    expect(script).toContain("tinytop.pid");
  });

  test("builds the Windows collector feature explicitly", () => {
    expect(script).toContain("--no-default-features");
    expect(script).toContain("--features");
    expect(script).toContain("windows-collector");
  });

  test("has explicit Windows service commands with elevation guidance", () => {
    expect(script).toContain("function Install-TinyTopService");
    expect(script).toContain("function Test-TinyTopAdmin");
    expect(script).toContain("New-Service");
    expect(script).toContain("Start-Service");
    expect(script).toContain("Stop-Service");
    expect(script).toContain("Run PowerShell as Administrator");
  });

  test("does not mention systemd as the Windows service mechanism", () => {
    expect(script).not.toContain("systemd");
  });
});
```

- [ ] **Step 2: Write failing Bash Windows feature-selection tests**

Add tests to `tests/tinytop-script.test.ts` that run `./tinytop --plain rust build --print-command` with `TINYTOP_RELEASE_OS=windows` and assert the command includes `--no-default-features --features windows-collector`.

- [ ] **Step 3: Verify red**

Run:

```bash
bun test tests/tinytop-powershell.test.ts tests/tinytop-script.test.ts
```

Expected: fails because `tinytop.ps1` does not exist and `rust build --print-command` is not implemented.

## Task 2: Implement Windows PowerShell Command Center

**Files:**
- Create: `tinytop.ps1`
- Modify: `tinytop`

**Interfaces:**
- `tinytop.ps1 rust install-binary`
- `tinytop.ps1 rust build`
- `tinytop.ps1 start|stop|restart|status|logs`
- `tinytop.ps1 service install|uninstall|start|stop|restart|status`
- `./tinytop rust build --print-command`

- [ ] **Step 1: Add PowerShell script**

Create `tinytop.ps1` with:

- strict mode
- version constant matching `VERSION`
- release asset naming for Windows
- local binary path under `$env:LOCALAPPDATA\TinyTop\bin\tinytop-agent.exe`
- state path under `$env:LOCALAPPDATA\TinyTop\state`
- logs under `$env:LOCALAPPDATA\TinyTop\logs`
- `Start-Process` based foreground/background start
- PID file cleanup
- `/api/version` status probe
- Windows service commands wrapping `New-Service`, `Start-Service`, `Stop-Service`, `Get-Service`, and `sc.exe delete`

- [ ] **Step 2: Update Bash wrapper build command**

Add `rust_build_args` and support `./tinytop rust build --print-command`. Select:

- Linux default: `cargo build --release --manifest-path agent/Cargo.toml -p tinytop-agent`
- macOS override: add `--no-default-features --features macos-collector`
- Windows override: add `--no-default-features --features windows-collector`

Use `TINYTOP_RELEASE_OS` only for tests and cross-shell simulation.

- [ ] **Step 3: Verify green**

Run:

```bash
bun test tests/tinytop-powershell.test.ts tests/tinytop-script.test.ts
shellcheck tinytop
cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector
```

Expected: tests pass, shellcheck passes, Windows collector crate check passes.

## Task 3: Make Critical Status Visually Obvious

**Files:**
- Modify: `legacy/dashboard/styles.css`
- Modify: `agent/assets/dashboard/styles.css`
- Modify: `tests/dashboard-operator-alert.test.ts`

**Interfaces:**
- Existing `data-status="critical"` on `#operator-status`
- Existing `#operator-state` visible text

- [ ] **Step 1: Write failing CSS tests**

Extend `tests/dashboard-operator-alert.test.ts` to assert the stylesheet contains:

- `.operator-status[data-status="critical"]`
- `.operator-status[data-status="critical"] > div`
- `.operator-status[data-status="critical"] #operator-state`
- `.operator-status[data-status="warning"]`
- `.operator-status[data-status="stale"]`

- [ ] **Step 2: Verify red**

Run:

```bash
bun test tests/dashboard-operator-alert.test.ts
```

Expected: fails because the stronger selectors are absent.

- [ ] **Step 3: Add stronger state styling**

Add CSS so the whole operator strip changes tone by status:

- Critical: stronger red border, darker red cell fill, visible red state pill, subtle inset glow.
- Warning: stronger amber treatment.
- Stale: muted/gray treatment with visible state pill.
- Healthy: mild green treatment.

Keep text labels and state names visible so the warning is not color-only.

- [ ] **Step 4: Keep asset parity**

Copy the same CSS to `agent/assets/dashboard/styles.css` or edit both trees identically.

- [ ] **Step 5: Verify green**

Run:

```bash
bun test tests/dashboard-operator-alert.test.ts tests/dashboard-assets.test.ts
diff -qr agent/assets/dashboard legacy/dashboard
```

Expected: tests pass and no asset differences.

## Task 4: Documentation, Version, Verification, Release

**Files:**
- Modify: `README.md`
- Modify: `INSTALL.md`
- Modify: `ARCHITECTURE.md`
- Modify: `PROGRESS.md`
- Modify: `CHANGELOG.md`
- Modify: `HANDOFF.md`
- Add: `docs/guides/WINDOWS.md`
- Add: `docs/reports/2026-06-26-windows-command-center-and-critical-status.md`
- Modify: `VERSION`
- Modify: `package.json`
- Modify: `tinytop`
- Modify: `agent/crates/*/Cargo.toml`
- Modify: `agent/Cargo.lock`

**Interfaces:**
- New version: `0.1.29`
- Tag: `v0.1.29`

- [ ] **Step 1: Update docs**

Document:

- `tinytop.ps1` is the Windows entrypoint.
- Windows can run foreground/background through PowerShell.
- Windows service commands exist and require Administrator for install/uninstall.
- Scoop/winget remain future packaging layers after Windows release assets are published.
- Current release may only include Linux asset until a Windows `.exe` is built and uploaded.

- [ ] **Step 2: Add report**

Create `docs/reports/2026-06-26-windows-command-center-and-critical-status.md` with scope, implementation notes, service answer, and verification evidence.

- [ ] **Step 3: Bump version**

Bump product and crate versions from `0.1.28` to `0.1.29`.

- [ ] **Step 4: Full verification**

Run:

```bash
bun test tests/tinytop-powershell.test.ts tests/tinytop-script.test.ts tests/dashboard-operator-alert.test.ts tests/dashboard-assets.test.ts
./tinytop check
cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector
cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings
git diff --check
bun audit
cargo audit --file agent/Cargo.lock
```

- [ ] **Step 5: Commit, tag, push, release**

Commit with Michel's author email and Codex trailer, tag `v0.1.29`, push `main` and tag, create a GitHub release, upload the Linux binary and checksum. Do not claim Windows release binary availability unless a real Windows `.exe` has been built and uploaded.

## Self-Review

- Spec coverage: The plan covers the PowerShell installer/control layer, Windows build selection, explicit Windows service path, critical status visibility, docs, versioning, and release.
- Placeholder scan: No placeholders remain; future package-manager work is explicitly out of this slice.
- Type consistency: Script function names and tests use the same command names.
