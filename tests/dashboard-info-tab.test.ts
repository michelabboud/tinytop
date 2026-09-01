import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { describeDiskCoverage } from "../agent/assets/dashboard/ladder-rules.js";

const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
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

/** The markup of the dashboard's history section, i.e. everything outside the dialog. */
const historySection = html.slice(
  html.indexOf('<section class="panel history-panel"'),
  html.indexOf('<section class="metric-band"'),
);
const infoPanel = html.slice(
  html.indexOf('id="settings-panel-info"'),
  html.indexOf('<div class="settings-validation-summary"'),
);

describe("reference material lives in Settings, not on the dashboard", () => {
  test("the coverage, tier, service and event blocks left the history section", () => {
    for (const id of [
      "history-coverage",
      "history-ladder-coverage",
      "history-archive-status",
      "history-otel-status",
      "thermal-status",
      "history-marker-list",
    ]) {
      expect(historySection, `${id} still on the dashboard`).not.toContain(`id="${id}"`);
      expect(infoPanel, `${id} missing from Info`).toContain(`id="${id}"`);
    }
  });

  test("the chart keeps the two things that belong to it", () => {
    // The scrubbed sample's readout has no meaning away from the chart, and the
    // disk PRESSURE banner is an alert rather than reference material.
    expect(historySection).toContain('id="history-sample-values"');
    expect(historySection).toContain('id="history-disk-pressure"');
  });

  test("Info declares four groups and each has an empty state", () => {
    for (const sub of ["coverage", "tiers", "services", "events"]) {
      expect(infoPanel).toContain(`data-settings-subtab="${sub}"`);
      expect(infoPanel).toContain(`data-settings-subpanel="${sub}"`);
    }
    for (const id of ["info-tiers-unavailable", "info-services-unavailable", "info-events-empty"]) {
      expect(infoPanel).toContain(`id="${id}"`);
    }
  });
});

describe("one disk reading, two audiences", () => {
  function renderer() {
    const banner = { hidden: true, textContent: "", dataset: {} as Record<string, string> };
    const info = { hidden: true, textContent: "", dataset: {} as Record<string, string> };
    const render = new Function(
      "elements",
      "describeDiskCoverage",
      "setHidden",
      `${extractFunction(app, "renderDiskCoverage")}; return renderDiskCoverage;`,
    )(
      { historyDiskPressure: banner, infoDiskCheck: info },
      describeDiskCoverage,
      (node: { hidden: boolean } | undefined, hidden: boolean) => {
        if (node) node.hidden = hidden;
      },
    ) as (disk: unknown) => void;
    return { render, banner, info };
  }

  test("a healthy disk reports in Info and shows NOTHING on the dashboard", () => {
    // This is the whole point of the move: "184 GiB free; minimum 5.0 GiB" is
    // reference material, and it was taking up a row of the dashboard forever.
    const { render, banner, info } = renderer();
    render({ freeBytes: 197_000_000_000, minFreeBytes: 5_368_709_120, pressure: false });
    expect(banner.hidden).toBe(true);
    expect(info.hidden).toBe(false);
    expect(info.textContent).toContain("free");
    expect(info.dataset.status).toBe("healthy");
  });

  test("disk PRESSURE still shows on the dashboard, where it cannot be missed", () => {
    // The failure this test exists to prevent: moving the block wholesale and
    // burying a real warning two clicks deep inside a settings dialog.
    const { render, banner, info } = renderer();
    render({ freeBytes: 1_073_741_824, minFreeBytes: 5_368_709_120, pressure: true });
    expect(banner.hidden).toBe(false);
    expect(banner.dataset.status).toBe("critical");
    expect(banner.textContent).toContain("Disk pressure");
    expect(banner.textContent).toContain("Shrink history or free disk");
    // ...and it is still reported in Info too, so the tab is never misleading.
    expect(info.hidden).toBe(false);
    expect(info.dataset.status).toBe("critical");
  });

  test("an unmeasured disk hides both rather than printing a blank row", () => {
    const { render, banner, info } = renderer();
    render(undefined);
    expect({ banner: banner.hidden, info: info.hidden }).toEqual({ banner: true, info: true });
    render({ freeBytes: null, minFreeBytes: 5_368_709_120, pressure: false });
    expect(info.hidden).toBe(false);
    expect(info.textContent).toContain("not measured yet");
    expect(banner.hidden).toBe(true);
  });
});

describe("Info groups say so when there is nothing to show", () => {
  test("an absent ladder shows the tier empty state instead of a bare legend", () => {
    const coverage = { replaceChildren() {}, hidden: false };
    const note = { hidden: true };
    const render = new Function(
      "elements",
      "state",
      "setHidden",
      "formatCoverageTime",
      "document",
      `${extractFunction(app, "renderTierCoverage")}; return renderTierCoverage;`,
    )(
      { historyLadderCoverage: coverage, infoTiersUnavailable: note },
      { retentionLadderAvailable: false },
      (node: { hidden: boolean } | undefined, hidden: boolean) => {
        if (node) node.hidden = hidden;
      },
      () => "-",
      { createElement: () => ({ append() {}, textContent: "" }) },
    ) as (tiers: unknown) => void;

    render(null);
    expect({ coverageHidden: coverage.hidden, noteHidden: note.hidden }).toEqual({
      coverageHidden: true,
      noteHidden: false,
    });
  });

  test("the services empty state is decided from the DOM, after all three lines render", () => {
    // Asked of the rendered nodes rather than of the coverage payload, so the
    // note can never disagree with what is actually on screen.
    const sync = new Function(
      "elements",
      "setHidden",
      `${extractFunction(app, "syncInfoServicesEmptyState")}; return syncInfoServicesEmptyState;`,
    );
    const run = (states: boolean[]) => {
      const note = { hidden: false };
      sync(
        {
          historyArchiveStatus: { hidden: states[0] },
          historyOtelStatus: { hidden: states[1] },
          historyThermalStatus: { hidden: states[2] },
          infoServicesUnavailable: note,
        },
        (node: { hidden: boolean } | undefined, hidden: boolean) => {
          if (node) node.hidden = hidden;
        },
      )();
      return note.hidden;
    };
    expect(run([true, true, true])).toBe(false); // nothing shown -> note visible
    expect(run([false, true, true])).toBe(true); // one shown -> note hidden
    expect(run([false, false, false])).toBe(true);
  });
});

describe("Info names the process answering the page", () => {
  function renderer() {
    const node = () => ({ textContent: "-" });
    const elements = {
      infoDaemonPid: node(),
      infoDaemonBind: node(),
      infoDaemonRuntime: node(),
      infoDaemonExecutable: node(),
      infoDaemonDatabase: node(),
    };
    const render = new Function(
      "elements",
      "setText",
      `${extractFunction(app, "renderDaemonProcess")}; return renderDaemonProcess;`,
    )(elements, (n: { textContent: string } | undefined, v: string) => {
      if (n) n.textContent = v;
    }) as (metadata: unknown) => void;
    return { render, elements };
  }

  test("the pid and the bound address are shown as reported", () => {
    const { render, elements } = renderer();
    render({
      runtime: "rust",
      daemon: {
        os: "linux",
        arch: "x86_64",
        pid: 4242,
        bind: { host: "127.0.0.1", port: 4274 },
        install: { executable: "/usr/local/bin/tinytop-agent" },
        storage: { sqlitePath: "/home/m/.local/share/tinytop/history.sqlite" },
      },
    });
    expect(elements.infoDaemonPid.textContent).toBe("4242");
    expect(elements.infoDaemonBind.textContent).toBe("127.0.0.1:4274");
    expect(elements.infoDaemonRuntime.textContent).toBe("rust · linux · x86_64");
    expect(elements.infoDaemonExecutable.textContent).toBe("/usr/local/bin/tinytop-agent");
    expect(elements.infoDaemonDatabase.textContent).toBe("/home/m/.local/share/tinytop/history.sqlite");
  });

  test("port 0 and pid 0 are still VALUES, not missing fields", () => {
    // `?? "not reported"` would have been wrong here: a falsy number is a number.
    const { render, elements } = renderer();
    render({ daemon: { pid: 0, bind: { host: "0.0.0.0", port: 0 } } });
    expect(elements.infoDaemonPid.textContent).toBe("0");
    expect(elements.infoDaemonBind.textContent).toBe("0.0.0.0:0");
  });

  test("an older daemon without a pid says so instead of printing a dash", () => {
    const { render, elements } = renderer();
    render({ runtime: "bun", daemon: { os: "linux", arch: "x86_64", bind: { host: "127.0.0.1", port: 4274 } } });
    expect(elements.infoDaemonPid.textContent).toBe("not reported");
    expect(elements.infoDaemonBind.textContent).toBe("127.0.0.1:4274");
    expect(elements.infoDaemonExecutable.textContent).toBe("not reported");
  });

  test("a failed version fetch reports every field as unavailable", () => {
    const { render, elements } = renderer();
    render(null);
    for (const key of Object.keys(elements) as Array<keyof typeof elements>) {
      expect(elements[key].textContent).toBe("not reported");
    }
  });

  test("the daemon exposes its pid, and the fixture does not fake it away", () => {
    const writer = readFileSync("agent/crates/tinytop-agent/src/writer.rs", "utf8");
    expect(writer).toContain("pid: std::process::id(),");
    expect(writer).toMatch(/struct DaemonMetadata \{[\s\S]*?pid: u32,/u);
  });
});
