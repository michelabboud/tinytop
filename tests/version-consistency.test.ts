import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

// VERSION is the single source of truth (global rule 12). Four other files
// restate it, and NOTHING checked that they agreed — so when 0.9.0 bumped
// VERSION alone, the crates stayed at 0.8.2 and `env!("CARGO_PKG_VERSION")`
// kept stamping 0.8.2 into the OTel `service.version` resource attribute and
// into every exported settings document, while `/api/version` correctly
// reported 0.9.0. A release misreporting which build produced an export is
// exactly the kind of quiet divergence a test should own rather than a habit.
const version = readFileSync("VERSION", "utf8").trim();

const CRATES = ["tinytop-agent", "tinytop-types", "tinytop-store", "tinytop-collectors"];

describe("every restatement of the version agrees with the VERSION file", () => {
  test("VERSION itself is a bare semver string", () => {
    expect(version).toMatch(/^\d+\.\d+\.\d+$/u);
  });

  test.each(CRATES)("crate %s is at the VERSION file's version", (crate) => {
    const manifest = readFileSync(`agent/crates/${crate}/Cargo.toml`, "utf8");
    // The package's own version is the FIRST `version =` in the file; a
    // dependency's version further down must not be mistaken for it.
    const declared = /^\s*version\s*=\s*"([^"]+)"/mu.exec(manifest)?.[1];
    expect(declared).toBe(version);
  });

  test("the POSIX wrapper's fallback version matches", () => {
    const wrapper = readFileSync("tinytop", "utf8");
    expect(/^TINYTOP_FALLBACK_VERSION="([^"]+)"/mu.exec(wrapper)?.[1]).toBe(version);
  });

  test("the PowerShell wrapper's fallback version matches", () => {
    const wrapper = readFileSync("tinytop.ps1", "utf8");
    expect(/^\$TinyTopFallbackVersion\s*=\s*"([^"]+)"/mu.exec(wrapper)?.[1]).toBe(version);
  });

  test("the newest CHANGELOG section is this version", () => {
    // Catches the other half of a half-done release: bumped files, no entry.
    const changelog = readFileSync("CHANGELOG.md", "utf8");
    expect(/^## (\d+\.\d+\.\d+)/mu.exec(changelog)?.[1]).toBe(version);
  });
});
