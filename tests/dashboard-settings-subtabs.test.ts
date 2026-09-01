import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  metricFamilyKeys,
  moveWithinTabRow,
  resolveTabInRow,
} from "../agent/assets/dashboard/ladder-rules.js";

const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
const styles = readFileSync("agent/assets/dashboard/styles.css", "utf8");

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

/** Every `<button ... data-settings-subtab="x" data-subtab-parent="y" ...>` in the static markup. */
function staticSubTabs(): Array<{ tag: string; name: string; parent: string }> {
  return (html.match(/<button[^>]*data-settings-subtab="[^"]*"[^>]*>/gu) ?? []).map((tag) => ({
    tag,
    name: /data-settings-subtab="([^"]*)"/u.exec(tag)?.[1] ?? "",
    parent: /data-subtab-parent="([^"]*)"/u.exec(tag)?.[1] ?? "",
  }));
}

function staticSubPanels(): Array<{ tag: string; name: string; parent: string }> {
  return (html.match(/<div[^>]*data-settings-subpanel="[^"]*"[^>]*>/gu) ?? []).map((tag) => ({
    tag,
    name: /data-settings-subpanel="([^"]*)"/u.exec(tag)?.[1] ?? "",
    parent: /data-subtab-parent="([^"]*)"/u.exec(tag)?.[1] ?? "",
  }));
}

describe("secondary tab rows are their own keyboard scope", () => {
  const row = ["tiers", "archive", "disk"];

  test("arrows wrap inside the row and Home or End jump to its boundary", () => {
    expect(moveWithinTabRow("tiers", "ArrowLeft", row)).toBe("disk");
    expect(moveWithinTabRow("disk", "ArrowRight", row)).toBe("tiers");
    expect(moveWithinTabRow("archive", "Home", row)).toBe("tiers");
    expect(moveWithinTabRow("archive", "End", row)).toBe("disk");
  });

  test("movement can NEVER leave the row it was given", () => {
    // This is the guarantee that keeps the two tablists separate scopes: no key,
    // from no starting point -- including a stale or foreign name -- can return
    // something that is not a member of the row that was passed in.
    for (const key of ["ArrowLeft", "ArrowRight", "Home", "End", "ArrowUp", "Enter"]) {
      for (const current of [...row, "general", "advanced", "", undefined, null]) {
        expect(row).toContain(moveWithinTabRow(current as string, key, row));
      }
    }
  });

  test("an empty row yields null instead of a primary tab name", () => {
    // The Metrics row does not exist until the daemon's registry is fetched.
    // moveSettingsTab would answer "general" here, which is a PRIMARY tab and
    // would select a panel in the wrong scope.
    expect(moveWithinTabRow("cpu", "ArrowRight", [])).toBeNull();
    expect(moveWithinTabRow("cpu", "Home", [])).toBeNull();
    expect(resolveTabInRow("cpu", [])).toBeNull();
    expect(resolveTabInRow("cpu", null as unknown as string[])).toBeNull();
  });

  test("a remembered sub-tab that no longer exists falls back to the row's first member", () => {
    expect(resolveTabInRow("archive", row)).toBe("archive");
    expect(resolveTabInRow("gone", row)).toBe("tiers");
    expect(resolveTabInRow(undefined as unknown as string, row)).toBe("tiers");
  });
});

describe("metric family keys are safe to put in a DOM id", () => {
  test("families are folded to [a-z0-9-] and never left empty", () => {
    expect(metricFamilyKeys(["CPU", "Memory", "File System"])).toEqual(["cpu", "memory", "file-system"]);
    expect(metricFamilyKeys(["  ", "***"])).toEqual(["other", "other-2"]);
  });

  test("two families that fold to the same key are disambiguated, never collided", () => {
    // A duplicate id would point two tabs at one panel and leave the other
    // panel permanently unreachable -- silently, with no error anywhere.
    expect(metricFamilyKeys(["CPU", "cpu", "  CPU  "])).toEqual(["cpu", "cpu-2", "cpu-3"]);
    // Separators are folded, not stripped, so these stay genuinely distinct.
    expect(metricFamilyKeys(["c.p.u", "cpu"])).toEqual(["c-p-u", "cpu"]);
  });

  test("non-array and malformed input degrade to an empty list rather than throwing", () => {
    expect(metricFamilyKeys(null as unknown as string[])).toEqual([]);
    expect(metricFamilyKeys([undefined as unknown as string])).toEqual(["other"]);
  });
});

describe("the secondary tab markup is internally consistent", () => {
  test("General and History declare their sub-groups", () => {
    const byParent = new Map<string, string[]>();
    for (const tab of staticSubTabs()) {
      byParent.set(tab.parent, [...(byParent.get(tab.parent) ?? []), tab.name]);
    }
    expect(byParent.get("general")).toEqual(["browser", "daemon", "thresholds", "display"]);
    expect(byParent.get("history")).toEqual(["tiers", "archive", "disk"]);
    expect(byParent.get("advanced")).toEqual(["otel", "document"]);
    // Metrics is built at runtime from METRIC_REGISTRY, so it has no static tabs.
    expect(byParent.has("metrics")).toBe(false);
    expect(html).toContain('id="metrics-settings-subtabs"');
  });

  test("each Advanced sub-tab is hidden with the group it selects", () => {
    // Either half of Advanced can be absent by runtime capability. A sub-tab
    // left visible over a hidden group would select an empty panel.
    expect(app).toContain("setHidden(elements.advancedOtelSubTab, !state.otelAvailable)");
    expect(app).toContain("setHidden(elements.advancedDocumentSubTab, !state.settingsDocumentAvailable)");
    // ...and the hiding must happen BEFORE the sync that re-resolves each row.
    const sync = extractFunction(app, "syncRetentionLadderAvailability");
    expect(sync.indexOf("advancedDocumentSubTab")).toBeLessThan(sync.indexOf("syncSettingsTabAvailability()"));
  });

  test("every sub-tab points at a sub-panel that exists, and back again", () => {
    const panels = staticSubPanels();
    expect(panels.length).toBeGreaterThan(0);
    for (const tab of staticSubTabs()) {
      const tabId = /id="([^"]*)"/u.exec(tab.tag)?.[1];
      const controls = /aria-controls="([^"]*)"/u.exec(tab.tag)?.[1];
      expect(tabId).toBeTruthy();
      expect(controls).toBeTruthy();
      expect(tab.tag).toContain('role="tab"');

      const panel = panels.find((candidate) => /id="([^"]*)"/u.exec(candidate.tag)?.[1] === controls);
      expect(panel).toBeDefined();
      expect(panel?.name).toBe(tab.name);
      expect(panel?.parent).toBe(tab.parent);
      expect(panel?.tag).toContain('role="tabpanel"');
      expect(panel?.tag).toContain(`aria-labelledby="${tabId}"`);
    }
  });

  test("exactly one sub-tab per row starts selected and is the row's only tab stop", () => {
    const selectedByParent = new Map<string, string[]>();
    for (const tab of staticSubTabs()) {
      const selected = /aria-selected="true"/u.test(tab.tag);
      if (selected) {
        selectedByParent.set(tab.parent, [...(selectedByParent.get(tab.parent) ?? []), tab.name]);
      }
      // Roving tabindex: Tab reaches the row once, then arrows move within it.
      expect(tab.tag).toContain(selected ? 'tabindex="0"' : 'tabindex="-1"');
    }
    expect(selectedByParent.get("general")).toEqual(["browser"]);
    expect(selectedByParent.get("history")).toEqual(["tiers"]);
  });

  test("every group help button is a real button wired to text that exists in the DOM", () => {
    const toggles = html.match(/<button[^>]*class="settings-help-toggle"[^>]*>/gu) ?? [];
    expect(toggles.length).toBeGreaterThanOrEqual(8);
    for (const toggle of toggles) {
      // A `title` tooltip would be invisible to touch and inconsistently
      // announced; the help must be real text, collapsed by `hidden`.
      expect(toggle).toContain('type="button"');
      expect(toggle).toContain('aria-expanded="false"');
      expect(toggle).toContain("aria-label=");
      const controls = /aria-controls="([^"]*)"/u.exec(toggle)?.[1];
      expect(controls).toBeTruthy();
      expect(html).toContain(`id="${controls}"`);
      expect(html).toMatch(new RegExp(`id="${controls}"[^>]*hidden`, "u"));
    }
  });
});

describe("a sub-panel is hidden, never unmounted", () => {
  function fakeRow(parent: string, names: string[]) {
    const tabs = names.map((name) => ({
      dataset: { settingsSubtab: name, subtabParent: parent },
      hidden: false,
      tabIndex: -1,
      attributes: new Map<string, string>(),
      focused: 0,
      setAttribute(key: string, value: string) {
        this.attributes.set(key, value);
      },
      focus() {
        this.focused += 1;
      },
    }));
    const panels = names.map((name) => ({
      dataset: { settingsSubpanel: name, subtabParent: parent },
      hidden: false,
      attached: true,
    }));
    return { tabs, panels };
  }

  function selector(row: ReturnType<typeof fakeRow>, state: Record<string, any>, persisted: string[]) {
    return new Function(
      "state",
      "settingsSubTabsFor",
      "settingsSubPanelsFor",
      "resolveTabInRow",
      "setHidden",
      "persistSettingsSubTab",
      `${extractFunction(app, "selectSettingsSubTab")}; return selectSettingsSubTab;`,
    )(
      state,
      () => row.tabs.filter((tab) => !tab.hidden),
      () => row.panels,
      resolveTabInRow,
      (node: { hidden: boolean }, hidden: boolean) => {
        node.hidden = hidden;
      },
      (_parent: string, name: string) => persisted.push(name),
    ) as (parent: string, requested?: string, options?: Record<string, unknown>) => string | null;
  }

  test("selecting a sub-tab only flips `hidden` -- every panel stays in the document", () => {
    // The load-bearing property. collectDaemonSettingsFromForm() reads through
    // the id-based `elements` cache, and a DETACHED input still answers
    // `.value` -- so unmounting a sub-panel would keep saving, silently, with
    // whatever the field held when it left the document.
    const row = fakeRow("history", ["tiers", "archive", "disk"]);
    const state = { activeSettingsSubTabs: {} as Record<string, string> };
    const select = selector(row, state, []);

    for (const name of ["archive", "disk", "tiers"]) {
      expect(select("history", name)).toBe(name);
      expect(row.panels.every((panel) => panel.attached)).toBe(true);
      expect(row.panels.filter((panel) => !panel.hidden).map((panel) => panel.dataset.settingsSubpanel)).toEqual([name]);
    }
  });

  test("selection sets a roving tabindex and remembers the row per parent", () => {
    const row = fakeRow("general", ["browser", "daemon", "thresholds", "display"]);
    const state = { activeSettingsSubTabs: { history: "archive" } as Record<string, string> };
    const persisted: string[] = [];
    const select = selector(row, state, persisted);

    expect(select("general", "thresholds")).toBe("thresholds");
    expect(row.tabs.map((tab) => tab.tabIndex)).toEqual([-1, -1, 0, -1]);
    expect(row.tabs.map((tab) => tab.attributes.get("aria-selected"))).toEqual(["false", "false", "true", "false"]);
    // The other parent's memory is untouched.
    expect(state.activeSettingsSubTabs).toEqual({ history: "archive", general: "thresholds" });
    expect(persisted).toEqual(["thresholds"]);
  });

  test("a sync pass does not persist, so an unvisited row keeps its remembered value", () => {
    const row = fakeRow("general", ["browser", "daemon"]);
    const persisted: string[] = [];
    const select = selector(row, { activeSettingsSubTabs: {} }, persisted);
    expect(select("general", "daemon", { persist: false })).toBe("daemon");
    expect(persisted).toEqual([]);
  });

  test("an empty row is a no-op rather than an exception", () => {
    const row = fakeRow("metrics", []);
    const select = selector(row, { activeSettingsSubTabs: {} }, []);
    expect(select("metrics", "cpu")).toBeNull();
  });

  test("nothing in the dashboard removes or replaces a secondary panel", () => {
    expect(app).not.toMatch(/settingsSubPanelsFor\([^)]*\)[^;]*\.remove\(\)/u);
    expect(app).toMatch(/for \(const panel of settingsSubPanelsFor\(parent\)\) \{\s*setHidden\(/u);
  });
});

describe("hidden sub-panels actually disappear", () => {
  test("every sub-tab class that sets `display` also restates `[hidden]`", () => {
    // Setting `display` on an element defeats the `hidden` attribute's own
    // `display: none`. These panels are hidden by attribute ONLY, so a missing
    // `[hidden]` rule leaves every group painted on top of every other one.
    expect(styles).toMatch(/\.settings-subtab-panel\s*\{[^}]*display:\s*grid/u);
    expect(styles).toMatch(/\.settings-subtab\[hidden\],\s*\.settings-subtab-panel\[hidden\]\s*\{[^}]*display:\s*none/u);
    expect(styles).toMatch(/\.history-ladder-shell\s*\{[^}]*display:\s*grid/u);
    expect(styles).toMatch(/\.history-ladder-shell\[hidden\]\s*\{[^}]*display:\s*none/u);
  });

  test("the ladder wrapper keeps the id the availability sync hides", () => {
    // syncRetentionLadderAvailability() hides the whole ladder in one move on a
    // runtime that has none; splitting it into three fieldsets must not have
    // left that call pointing at only one of them.
    expect(html).toMatch(/<div class="history-ladder-shell" id="history-ladder-settings-group">/u);
    expect(app).toContain("setHidden(elements.historyLadderSettingsGroup, !state.retentionLadderAvailable)");
    // Matched on the class TOKEN, not an exact attribute string: the tiers
    // fieldset carries a second class and an exact match silently dropped it.
    expect((html.match(/class="[^"]*\bhistory-ladder-settings-group\b[^"]*"/gu) ?? []).length).toBe(3);
  });
});

describe("group help toggles", () => {
  function toggler(dom: Record<string, { hidden: boolean }>) {
    return new Function(
      "document",
      "setHidden",
      `${extractFunction(app, "toggleSettingsHelp")}; return toggleSettingsHelp;`,
    )(
      { getElementById: (id: string) => dom[id] ?? null },
      (node: { hidden: boolean }, hidden: boolean) => {
        node.hidden = hidden;
      },
    ) as (button: unknown) => void;
  }

  function button(controls: string, expanded = false) {
    const attributes = new Map<string, string>([
      ["aria-controls", controls],
      ["aria-expanded", String(expanded)],
    ]);
    return {
      attributes,
      getAttribute: (key: string) => attributes.get(key) ?? null,
      setAttribute: (key: string, value: string) => attributes.set(key, value),
    };
  }

  test("expanding shows the text and collapsing hides it again", () => {
    const help = { hidden: true };
    const toggle = toggler({ "help-history-tiers": help });
    const control = button("help-history-tiers");

    toggle(control);
    expect(control.getAttribute("aria-expanded")).toBe("true");
    expect(help.hidden).toBe(false);

    toggle(control);
    expect(control.getAttribute("aria-expanded")).toBe("false");
    expect(help.hidden).toBe(true);
  });

  test("a button pointing at missing text changes nothing rather than throwing", () => {
    const toggle = toggler({});
    const control = button("help-that-was-removed");
    expect(() => toggle(control)).not.toThrow();
    // The state must NOT advance: an aria-expanded="true" with nothing to show
    // would announce content that is not there.
    expect(control.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("secondary tab wiring", () => {
  test("sub-tabs and help are delegated on the dialog, not bound per button", () => {
    // The Metrics row is rebuilt on every registry fetch; per-button listeners
    // would have to be rebound each time or leak.
    expect(app).toMatch(/settingsDialog\?\.addEventListener\("click",[\s\S]{0,400}data-settings-subtab/u);
    expect(app).toMatch(/settingsDialog\?\.addEventListener\("focusin"/u);
    expect(app).toMatch(/settingsDialog\?\.addEventListener\("keydown",[\s\S]{0,600}moveWithinTabRow/u);
  });

  test("the arrow handler bails out before preventDefault when focus is not on a sub-tab", () => {
    // Otherwise it would swallow the primary row's own arrow keys and the two
    // tablists would fight over the same four keys.
    const handler = /settingsDialog\?\.addEventListener\("keydown", \(event\) => \{([\s\S]*?)\n\}\);/u.exec(app)?.[1] ?? "";
    expect(handler).toContain("data-settings-subtab");
    expect(handler.indexOf("if (!subTab) return;")).toBeGreaterThan(0);
    expect(handler.indexOf("if (!subTab) return;")).toBeLessThan(handler.indexOf("event.preventDefault()"));
  });

  test("the remembered selection survives a reload and is stored defensively", () => {
    expect(app).toContain('settingsSubTabs: "tinytop.settingsSubTabs"');
    expect(app).toMatch(/function readStoredSettingsSubTabs\(\) \{[\s\S]*?Array\.isArray\(stored\)/u);
    expect(app).toMatch(/function persistSettingsSubTab\([^)]*\) \{\s*storeJson\([^,]*, \{ \.\.\.readStoredSettingsSubTabs\(\)/u);
  });

  test("the Metrics row is rebuilt from the same grouping that builds its panels", () => {
    expect(app).toMatch(/const groups = groupMetricRegistry\(state\.metricRegistry\);/u);
    expect(app).toMatch(/const familyKeys = metricFamilyKeys\(groups\.map\(\(group\) => group\.family\)\);/u);
    expect(app).toContain('elements.metricsSettingsSubtabs?.replaceChildren()');
    expect(app).toContain('panel.dataset.subtabParent = "metrics"');
  });
});
