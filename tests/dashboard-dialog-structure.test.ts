import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("agent/assets/dashboard/index.html", "utf8");

/**
 * Walk the raw markup tracking `<div>` depth and report the depth at which each
 * element of interest opens.
 *
 * A real parser would silently REPAIR a missing close tag, which is exactly how
 * this defect survived a release: the browser recovered, every panel still
 * rendered, and the only visible symptom was the dialog's action buttons being
 * laid out below its bottom edge on one tab. Reading the source depth catches
 * the mistake where it was made.
 */
function divDepths(source: string): Map<string, number> {
  const depths = new Map<string, number>();
  const token = /<div\b([^>]*)>|<\/div>/gu;
  let depth = 0;
  for (const match of source.matchAll(token)) {
    if (match[0] === "</div>") {
      depth -= 1;
      continue;
    }
    const attrs = match[1] ?? "";
    const id = /\bid="([^"]+)"/u.exec(attrs)?.[1];
    const cls = /\bclass="([^"]+)"/u.exec(attrs)?.[1];
    const key = id ? `#${id}` : cls ? `.${cls.split(/\s+/u)[0]}` : null;
    if (key && !depths.has(key)) depths.set(key, depth);
    if (!/\/>$/u.test(match[0])) depth += 1;
  }
  return depths;
}

const depths = divDepths(html);
const PANELS = [
  "#settings-panel-general",
  "#settings-panel-history",
  "#settings-panel-metrics",
  "#settings-panel-advanced",
  "#settings-panel-thermals",
  "#settings-panel-info",
];

describe("the settings dialog is nested the way its CSS assumes", () => {
  test("all five tab panels are siblings at the same depth", () => {
    // A panel nested INSIDE another panel inherits the parent's `[hidden]`, so
    // it can never be shown independently -- and it silently changes which
    // element the grid is sizing. Thermals ended up inside Advanced this way.
    const found = PANELS.map((panel) => [panel, depths.get(panel)] as const);
    for (const [panel, depth] of found) expect(depth, `${panel} missing`).toBeDefined();
    const unique = new Set(found.map(([, depth]) => depth));
    expect({ found: Object.fromEntries(found), distinctDepths: unique.size }).toEqual({
      found: Object.fromEntries(PANELS.map((panel) => [panel, depths.get(PANELS[0])])),
      distinctDepths: 1,
    });
  });

  test("every tab panel is one level inside the settings grid", () => {
    const grid = depths.get(".settings-grid");
    expect(grid).toBeDefined();
    for (const panel of PANELS) expect(depths.get(panel)).toBe((grid as number) + 1);
  });

  test("the validation summary is a sibling of the grid, not inside it", () => {
    expect(depths.get("#settings-validation-summary")).toBe(depths.get(".settings-grid"));
  });

  test("the action row is a sibling of the dialog body, not inside it", () => {
    // This is the one with teeth. `.settings-card` is a three-row grid --
    // header / body(1fr) / actions -- and the actions row only gets a height if
    // it is actually a CHILD of the card. Nested inside the scrolling body it
    // was laid out below the dialog's bottom edge, where Save, Cancel, Reset and
    // Defaults could not be clicked at all.
    expect(depths.get(".settings-dialog-actions")).toBe(depths.get(".settings-dialog-body"));
  });

  test("the whole document's div tags balance", () => {
    const opens = (html.match(/<div\b[^>]*>/gu) ?? []).filter((tag) => !/\/>$/u.test(tag)).length;
    const closes = (html.match(/<\/div>/gu) ?? []).length;
    expect({ opens, closes }).toEqual({ opens: closes, closes });
  });

  test("each secondary sub-panel sits inside the panel that owns it", () => {
    for (const [sub, parent] of [
      ["#settings-subpanel-general-browser", "#settings-panel-general"],
      ["#settings-subpanel-history-tiers", "#settings-panel-history"],
      ["#settings-subpanel-advanced-otel", "#settings-panel-advanced"],
      ["#settings-subpanel-advanced-document", "#settings-panel-advanced"],
    ] as const) {
      expect(depths.get(sub), `${sub} depth`).toBeGreaterThan(depths.get(parent) as number);
    }
  });
});
