import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  brokenSettingsControls,
  settingsIntegrityErrors,
} from "../agent/assets/dashboard/ladder-rules.js";

const app = readFileSync("agent/assets/dashboard/app.js", "utf8");

function extractFunction(source: string, name: string): string {
  let start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`${name} not found`);
  if (source.slice(start - 6, start) === "async ") start -= 6;
  const signatureEnd = source.indexOf(") {", start);
  const bodyStart = signatureEnd >= 0 ? signatureEnd + 2 : source.indexOf("{", start);
  let depth = 0;
  for (let index = bodyStart; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(start, index + 1);
  }
  throw new Error(`${name} is incomplete`);
}

function elementsReferencedBy(name: string): Set<string> {
  return new Set(Array.from(extractFunction(app, name).matchAll(/elements\.(\w+)/gu), (m) => m[1]));
}

const connected = { isConnected: true, value: "7" };
const detached = { isConnected: false, value: "stale" };

describe("the save manifest cannot drift from what the save reads", () => {
  test("every control collectDaemonSettingsFromForm reads is in the manifest, and vice versa", () => {
    // This is what makes the guard trustworthy rather than decorative. The
    // manifest is written by hand, so nothing stops a future edit from reading
    // a new control and forgetting to list it -- which would leave exactly the
    // silent hole the guard exists to close. Here the two sets are compared, so
    // that drift is a RED TEST instead of an invisible gap.
    const read = elementsReferencedBy("collectDaemonSettingsFromForm");
    const declared = elementsReferencedBy("daemonSettingsControlManifest");

    const unguarded = [...read].filter((name) => !declared.has(name)).sort();
    const stale = [...declared].filter((name) => !read.has(name)).sort();

    expect({ unguarded, stale }).toEqual({ unguarded: [], stale: [] });
    expect(read.size).toBeGreaterThan(40);
  });

  test("the manifest gates each control on the capability that makes it readable", () => {
    // An absent runtime feature must not be reported as a broken form: on a
    // daemon with no OTel there is no endpoint field, and that is correct.
    const manifest = extractFunction(app, "daemonSettingsControlManifest");
    expect(manifest).toContain("if (state.retentionLadderAvailable)");
    expect(manifest).toContain("if (state.otelAvailable)");
    expect(manifest).toContain("if (state.thermalAvailable)");
    expect(manifest).toContain("if (state.metricsAvailable)");
    // The legacy pair is read ONLY when there is no ladder; guarding it always
    // would fire on every modern daemon.
    expect(manifest).toMatch(/\} else \{[\s\S]*daemonRetentionHours/u);
  });
});

describe("a control that cannot be read is caught, not trusted", () => {
  test("a DETACHED node is reported even though it still answers .value", () => {
    // The whole point. `detached.value` is "stale" -- readable, plausible, and
    // not what the user is looking at.
    expect(detached.value).toBe("stale");
    expect(brokenSettingsControls([{ name: "L1 raw days", node: detached }])).toEqual([
      { name: "L1 raw days", reason: "detached" },
    ]);
  });

  test("a missing node is reported separately from a detached one", () => {
    expect(
      brokenSettingsControls([
        { name: "Gone", node: null },
        { name: "Also gone", node: undefined },
        { name: "Unmounted", node: detached },
        { name: "Fine", node: connected },
      ]),
    ).toEqual([
      { name: "Gone", reason: "missing" },
      { name: "Also gone", reason: "missing" },
      { name: "Unmounted", reason: "detached" },
    ]);
  });

  test("only `isConnected === true` counts as readable", () => {
    // Not truthiness: a node reporting undefined, or a string, is not a node
    // that has been confirmed to be in the document.
    for (const value of [undefined, "true", 1, {}, null]) {
      expect(brokenSettingsControls([{ name: "X", node: { isConnected: value } }])).toHaveLength(1);
    }
    expect(brokenSettingsControls([{ name: "X", node: connected }])).toEqual([]);
  });

  test("malformed input degrades instead of throwing", () => {
    expect(brokenSettingsControls(null as unknown as [])).toEqual([]);
    expect(brokenSettingsControls([null as unknown as { name: string; node: null }])).toEqual([
      { name: "(unnamed control)", reason: "missing" },
    ]);
  });
});

describe("the refusal message tells the user what happened and what to do", () => {
  test("a clean form produces no error at all", () => {
    expect(settingsIntegrityErrors([])).toEqual([]);
    expect(settingsIntegrityErrors(null as unknown as [])).toEqual([]);
  });

  test("it names every unreadable setting and says nothing was saved", () => {
    const [message] = settingsIntegrityErrors([
      { name: "L1 raw days", reason: "detached" },
      { name: "Cold after months", reason: "detached" },
      { name: "Endpoint", reason: "missing" },
    ]);
    expect(message).toContain("Nothing was saved");
    expect(message).toContain("L1 raw days");
    expect(message).toContain("Cold after months");
    expect(message).toContain("Endpoint");
    // The two causes read differently, because they mean different things.
    expect(message).toContain("removed from the page after it loaded");
    expect(message).toContain("never present on the page");
    // A bare "save failed" would leave someone retrying into the same silence.
    expect(message).toContain("Reload the dashboard");
    expect(message).toContain("3 settings");
  });

  test("one broken control reads as singular", () => {
    const [message] = settingsIntegrityErrors([{ name: "L4 days", reason: "detached" }]);
    expect(message).toContain("1 setting could not be read");
    expect(message).toContain("written a stale or default value for it");
  });
});

describe("validation refuses before it judges any value", () => {
  function validator(manifest: Array<{ name: string; node: unknown }>) {
    const rendered: string[][] = [];
    const validate = new Function(
      "settingsIntegrityErrors",
      "brokenSettingsControls",
      "daemonSettingsControlManifest",
      "renderSettingsValidation",
      "state",
      "validateRange",
      `${extractFunction(app, "validateDaemonSettings")}; return validateDaemonSettings;`,
    )(
      settingsIntegrityErrors,
      brokenSettingsControls,
      () => manifest,
      (errors: string[]) => rendered.push(errors),
      { retentionLadderAvailable: false, otelAvailable: false, thermalAvailable: false },
      () => {
        throw new Error("validateRange must not run while a control is unreadable");
      },
    ) as (settings: unknown) => string[];
    return { validate, rendered };
  }

  test("a detached control stops validation dead and surfaces one clear error", () => {
    // validateRange throws if reached: a range check on a detached input would
    // PASS -- it is checking the stale value -- and the save would then write it.
    const { validate, rendered } = validator([
      { name: "L1 raw days", node: detached },
      { name: "Refresh ms", node: connected },
    ]);
    const errors = validate({});
    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain("L1 raw days");
    expect(rendered).toEqual([errors]);
  });

  test("a healthy form falls through to the ordinary value checks", () => {
    const { validate } = validator([{ name: "Refresh ms", node: connected }]);
    // Reaching validateRange is the pass condition here -- it throws by design.
    expect(() => validate({ pollIntervalMs: 1500, targetDatabaseBytes: 0, topProcessCount: 5 })).toThrow(
      "validateRange must not run while a control is unreadable",
    );
  });
});

describe("an unreadable form never reaches the network", () => {
  test("saveDaemonSettings sends no PUT when validation refuses", async () => {
    // The end of the chain: the guard is worthless if the save ignores it.
    const calls: string[] = [];
    const statuses: string[] = [];
    const save = new Function(
      "collectDaemonSettingsFromForm",
      "validateDaemonSettings",
      "renderSettingsStatus",
      "fetch",
      `${extractFunction(app, "saveDaemonSettings")}; return saveDaemonSettings;`,
    )(
      () => ({}),
      () => settingsIntegrityErrors([{ name: "L1 raw days", reason: "detached" }]),
      (message: string) => statuses.push(message),
      (url: string) => {
        calls.push(url);
        throw new Error("the save must not reach the network");
      },
    ) as () => Promise<void>;

    await save();
    expect(calls).toEqual([]);
    expect(statuses).toEqual(["Fix validation errors before saving."]);
  });
});
