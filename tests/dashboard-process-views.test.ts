import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  DEFAULT_PROCESS_VIEW,
  NO_MEMORY_LIST_NOTICE,
  PROCESS_VIEWS,
  PROCESS_VIEW_DESCRIPTIONS,
  formatSwapBytes,
  naturalProcessSort,
  processCounterText,
  processListsFrom,
  processViewFor,
  sortProcessRows,
  swapBytesSortValue,
} from "../agent/assets/dashboard/ladder-rules.js";
import { makeSnapshot } from "./fixtures";

// Since schema v6 a sample is a UNION: the top N by CPU, then the memory-only
// members of the top N by memory (RSS + swap). The dashboard shows one list at
// a time and these rules are the only code that picks it (ADR 0036, ADR 0037).

type Row = {
  pid: number;
  command: string;
  cpuPercent: number;
  memoryPercent: number;
  rssBytes: number;
  swapBytes?: number;
  cpuRank?: number;
  memoryRank?: number;
  gpuPercent?: number;
};

const GIB = 1024 * 1024 * 1024;

function row(pid: number, extra: Partial<Row> = {}): Row {
  return { pid, command: `proc-${pid}`, cpuPercent: 0, memoryPercent: 0, rssBytes: 0, ...extra };
}

const pids = (rows: Array<{ pid: number }>) => rows.map((process) => process.pid);

// N = 3. CPU list: 10, 11, 12. Memory list: 20, 11, 21. 11 is in both; 20 is
// the swapped-out process the plan exists for (small RSS, large swap).
function union(): Row[] {
  return [
    row(10, { cpuPercent: 90, rssBytes: 1 * GIB, swapBytes: 0, cpuRank: 0 }),
    row(11, { cpuPercent: 50, rssBytes: 4 * GIB, swapBytes: 1 * GIB, cpuRank: 1, memoryRank: 1 }),
    row(12, { cpuPercent: 20, rssBytes: 0.2 * GIB, cpuRank: 2 }),
    row(20, { cpuPercent: 0, rssBytes: 0.1 * GIB, swapBytes: 6 * GIB, memoryRank: 0 }),
    row(21, { cpuPercent: 1, rssBytes: 3 * GIB, swapBytes: 0, memoryRank: 2 }),
  ];
}

describe("the two lists of one sample", () => {
  test("both lists come out of the union, each in its own rank order", () => {
    const lists = processListsFrom(union());
    expect(pids(lists.cpu)).toEqual([10, 11, 12]);
    expect(pids(lists.memory ?? [])).toEqual([20, 11, 21]);
  });

  test("rank order wins over the order the rows arrived in", () => {
    const shuffled = [union()[4], union()[2], union()[3], union()[0], union()[1]];
    const lists = processListsFrom(shuffled);
    expect(pids(lists.cpu)).toEqual([10, 11, 12]);
    expect(pids(lists.memory ?? [])).toEqual([20, 11, 21]);
  });

  test("the same row object is in both lists, not a copy", () => {
    const rows = union();
    const lists = processListsFrom(rows);
    expect(lists.cpu[1]).toBe(rows[1]);
    expect(lists.memory?.[1]).toBe(rows[1]);
  });

  test("a pre-v6 capture has a CPU list and NO memory list (null, not empty)", () => {
    // ADR 0037: read back with cpuRank == rank and no memoryRank anywhere.
    const capture = [row(1, { cpuRank: 0 }), row(2, { cpuRank: 1 }), row(3, { cpuRank: 2 })];
    const lists = processListsFrom(capture);
    expect(pids(lists.cpu)).toEqual([1, 2, 3]);
    expect(lists.memory).toBeNull();
  });

  test("an unranked row in a mixed capture is in neither list", () => {
    const mixed = [...union(), row(99, { cpuPercent: 99, rssBytes: 9 * GIB, swapBytes: 9 * GIB })];
    const lists = processListsFrom(mixed);
    expect(pids(lists.cpu)).not.toContain(99);
    expect(pids(lists.memory ?? [])).not.toContain(99);
    expect(lists.cpu).toHaveLength(3);
    expect(lists.memory).toHaveLength(3);
  });

  test("a rank that is not a non-negative integer is no rank", () => {
    const rows = [
      row(1, { cpuRank: 0, memoryRank: 0 }),
      row(2, { cpuRank: -1 }),
      row(3, { cpuRank: 1.5 }),
      row(4, { cpuRank: null as unknown as number, memoryRank: "0" as unknown as number }),
    ];
    const lists = processListsFrom(rows);
    expect(pids(lists.cpu)).toEqual([1]);
    expect(pids(lists.memory ?? [])).toEqual([1]);
  });

  test("N = 1: one row in both lists, or two rows with one list each", () => {
    const both = processListsFrom([row(1, { cpuRank: 0, memoryRank: 0 })]);
    expect(pids(both.cpu)).toEqual([1]);
    expect(pids(both.memory ?? [])).toEqual([1]);

    const split = processListsFrom([row(1, { cpuRank: 0 }), row(2, { memoryRank: 0 })]);
    expect(pids(split.cpu)).toEqual([1]);
    expect(pids(split.memory ?? [])).toEqual([2]);
  });

  test("equal ranks keep the order the rows arrived in", () => {
    // The collectors never emit a duplicate rank; if a sample ever carries
    // one, the table must not reshuffle from one poll to the next.
    const rows = [row(5, { cpuRank: 0 }), row(4, { cpuRank: 0 }), row(3, { cpuRank: 0 }), row(2, { memoryRank: 0 })];
    for (let run = 0; run < 5; run += 1) expect(pids(processListsFrom(rows).cpu)).toEqual([5, 4, 3]);
  });

  test("an empty or malformed sample is two empty lists and nothing to explain", () => {
    for (const sample of [[], undefined, null, "rows", { length: 2 }]) {
      expect(processListsFrom(sample)).toEqual({ cpu: [], memory: [] });
      expect(processViewFor(sample, "memory")).toEqual({ view: "memory", rows: [], memoryAvailable: true, notice: null });
    }
    expect(pids(processListsFrom([null, row(1, { cpuRank: 0 }), 7]).cpu)).toEqual([1]);
  });

  test("the input array is never reordered or mutated", () => {
    const rows = [row(2, { cpuRank: 1, memoryRank: 0 }), row(1, { cpuRank: 0, memoryRank: 1 })];
    const before = structuredClone(rows);
    processListsFrom(rows);
    processViewFor(rows, "memory");
    sortProcessRows(rows, { key: "pid", direction: "asc" });
    expect(rows).toEqual(before);
  });
});

describe("the list the table shows", () => {
  test("the default view is the CPU list, the one the table always showed", () => {
    expect(DEFAULT_PROCESS_VIEW).toBe("cpu");
    expect([...PROCESS_VIEWS]).toEqual(["cpu", "memory"]);
    const shown = processViewFor(union(), DEFAULT_PROCESS_VIEW);
    expect(shown.view).toBe("cpu");
    expect(pids(shown.rows)).toEqual([10, 11, 12]);
    expect(shown.memoryAvailable).toBe(true);
    expect(shown.notice).toBeNull();
  });

  test("the memory view shows the memory list", () => {
    const shown = processViewFor(union(), "memory");
    expect(shown.view).toBe("memory");
    expect(pids(shown.rows)).toEqual([20, 11, 21]);
    expect(shown.notice).toBeNull();
  });

  test("an unknown view is the CPU list", () => {
    for (const view of [undefined, null, "", "rss", "MEMORY"]) {
      expect(processViewFor(union(), view).view).toBe("cpu");
    }
  });

  test("asking for memory on a pre-v6 capture shows the CPU list and says why", () => {
    const capture = [row(1, { cpuRank: 0 }), row(2, { cpuRank: 1 })];
    const shown = processViewFor(capture, "memory");
    expect(shown.view).toBe("cpu");
    expect(pids(shown.rows)).toEqual([1, 2]);
    expect(shown.memoryAvailable).toBe(false);
    expect(shown.notice).toBe(NO_MEMORY_LIST_NOTICE);
    // The sentence is there in the CPU view too: it is what explains the
    // unavailable "By memory" button.
    expect(processViewFor(capture, "cpu").notice).toBe(NO_MEMORY_LIST_NOTICE);
  });

  test("the sentence is plain and says what is shown instead", () => {
    expect(NO_MEMORY_LIST_NOTICE).toBe(
      "This capture has no by-memory list: it was recorded without memory ranks or per-process swap, so its processes are shown by CPU.",
    );
  });

  test("the memory description says the ranking is RSS plus swap", () => {
    expect(PROCESS_VIEW_DESCRIPTIONS.cpu).toBe("Top local processes by CPU usage.");
    expect(PROCESS_VIEW_DESCRIPTIONS.memory).toBe(
      "Top local processes by memory: RSS plus swap, so a swapped-out process still ranks.",
    );
  });
});

describe("the legacy Bun collector's rows (no ranks, no swap)", () => {
  // src/collector.ts emits { pid, command, cpuPercent, memoryPercent, rssBytes }
  // in `ps --sort=-%cpu` order. The Bun server serves the same dashboard from
  // disk, so the same rules must give it a working table.
  const snapshot = JSON.parse(
    JSON.stringify(
      makeSnapshot({
        processes: [
          { pid: 300, command: "bun run dev", cpuPercent: 31.5, memoryPercent: 1.2, rssBytes: 200_000_000 },
          { pid: 100, command: "postgres", cpuPercent: 12, memoryPercent: 9.5, rssBytes: 1_500_000_000 },
          { pid: 200, command: "sleep 60", cpuPercent: 0, memoryPercent: 0, rssBytes: 800_000 },
        ],
      }),
    ),
  );

  test("the fixture really carries no rank and no swap", () => {
    for (const process of snapshot.processes) {
      expect(Object.keys(process).sort()).toEqual(["command", "cpuPercent", "memoryPercent", "pid", "rssBytes"]);
    }
  });

  test("the rows are the CPU list in the order received, and the table is not empty", () => {
    const shown = processViewFor(snapshot.processes, "cpu");
    expect(shown.view).toBe("cpu");
    expect(pids(shown.rows)).toEqual([300, 100, 200]);
    expect(processCounterText(shown.rows.length, shown.rows.length)).toBe("3 / 3 rows");
  });

  test("there is no by-memory view, with the same sentence as a pre-v6 capture", () => {
    const shown = processViewFor(snapshot.processes, "memory");
    expect(shown.view).toBe("cpu");
    expect(shown.memoryAvailable).toBe(false);
    expect(pids(shown.rows)).toEqual([300, 100, 200]);
    expect(shown.notice).toBe(NO_MEMORY_LIST_NOTICE);
  });

  test("every row's swap is a dash", () => {
    expect(snapshot.processes.map((process: Row) => formatSwapBytes(process.swapBytes))).toEqual(["—", "—", "—"]);
  });
});

describe("swap: unknown is a dash, zero is a zero", () => {
  test("an absent, null or malformed value is a dash, never 0", () => {
    for (const value of [undefined, null, Number.NaN, Number.POSITIVE_INFINITY, -1, "1024"]) {
      expect(formatSwapBytes(value)).toBe("—");
    }
    expect(formatSwapBytes(row(1).swapBytes)).toBe("—");
  });

  test("a present 0 is a measured zero", () => {
    expect(formatSwapBytes(0)).toBe("0 B");
    expect(formatSwapBytes(0)).not.toBe(formatSwapBytes(undefined));
  });

  test("real values use the table's byte units", () => {
    expect(formatSwapBytes(512)).toBe("512 B");
    expect(formatSwapBytes(1536 * 1024)).toBe("1.5 MiB");
    expect(formatSwapBytes(2.6 * GIB)).toBe("2.6 GiB");
    expect(formatSwapBytes(12 * GIB)).toBe("12 GiB");
  });

  test("unknown swap sorts below a real zero", () => {
    expect(swapBytesSortValue({ swapBytes: 0 })).toBe(0);
    expect(swapBytesSortValue({})).toBe(-1);
    expect(swapBytesSortValue({ swapBytes: null })).toBe(-1);
    expect(swapBytesSortValue({ swapBytes: 4096 })).toBe(4096);
  });
});

describe("column sorting works inside the shown list", () => {
  test("each view starts in its natural order", () => {
    expect(naturalProcessSort("cpu")).toEqual({ key: "cpu", direction: "desc" });
    expect(naturalProcessSort("memory")).toEqual({ key: "rank", direction: "desc" });
    expect(naturalProcessSort("nonsense")).toEqual({ key: "cpu", direction: "desc" });
    // A fresh object each time: the caller stores and mutates its own copy.
    expect(naturalProcessSort("cpu")).not.toBe(naturalProcessSort("cpu"));
  });

  test("the rank sort keeps the list's own order, in either direction", () => {
    const memory = processViewFor(union(), "memory").rows;
    expect(pids(sortProcessRows(memory, naturalProcessSort("memory")))).toEqual([20, 11, 21]);
    expect(pids(sortProcessRows(memory, { key: "rank", direction: "asc" }))).toEqual([20, 11, 21]);
    expect(pids(sortProcessRows(memory, { key: "nonsense", direction: "asc" }))).toEqual([20, 11, 21]);
    expect(pids(sortProcessRows(memory, undefined))).toEqual([20, 11, 21]);
  });

  test("the CPU view's natural sort reproduces the CPU rank order", () => {
    const cpu = processViewFor(union(), "cpu").rows;
    expect(pids(sortProcessRows(cpu, naturalProcessSort("cpu")))).toEqual([10, 11, 12]);
  });

  test("a column sort reorders only the rows of the shown list", () => {
    const memory = processViewFor(union(), "memory").rows;
    expect(pids(sortProcessRows(memory, { key: "rss", direction: "desc" }))).toEqual([11, 21, 20]);
    expect(pids(sortProcessRows(memory, { key: "swap", direction: "desc" }))).toEqual([20, 11, 21]);
    expect(pids(sortProcessRows(memory, { key: "pid", direction: "asc" }))).toEqual([11, 20, 21]);
    expect(pids(sortProcessRows(memory, { key: "cpu", direction: "desc" }))).toEqual([11, 21, 20]);
  });

  test("unknown swap goes last when sorting by swap descending, after a real zero", () => {
    const cpu = processViewFor(union(), "cpu").rows;
    // 11 has 1 GiB, 10 has a measured 0, 12 is unknown.
    expect(pids(sortProcessRows(cpu, { key: "swap", direction: "desc" }))).toEqual([11, 10, 12]);
    expect(pids(sortProcessRows(cpu, { key: "swap", direction: "asc" }))).toEqual([12, 10, 11]);
  });

  test("ties keep the list's order in both directions", () => {
    const rows = [row(3, { cpuRank: 0 }), row(1, { cpuRank: 1 }), row(2, { cpuRank: 2 })];
    expect(pids(sortProcessRows(rows, { key: "cpu", direction: "desc" }))).toEqual([3, 1, 2]);
    expect(pids(sortProcessRows(rows, { key: "cpu", direction: "asc" }))).toEqual([3, 1, 2]);
  });
});

describe("the counter describes the shown list, not the union", () => {
  test("the text", () => {
    expect(processCounterText(3, 3)).toBe("3 / 3 rows");
    expect(processCounterText(1, 12)).toBe("1 / 12 rows");
    expect(processCounterText(0, 0)).toBe("0 / 0 rows");
  });

  test("a union of five rows counts three in either view", () => {
    const rows = union();
    expect(rows).toHaveLength(5);
    for (const view of PROCESS_VIEWS) {
      const shown = processViewFor(rows, view);
      expect(processCounterText(shown.rows.length, shown.rows.length)).toBe("3 / 3 rows");
    }
  });
});

describe("the list control is a real control, and there is still one switch implementation", () => {
  const html = readFileSync("agent/assets/dashboard/index.html", "utf8");
  const app = readFileSync("agent/assets/dashboard/app.js", "utf8");
  const styles = readFileSync("agent/assets/dashboard/styles.css", "utf8");
  const group = /<div class="process-view-nav"[^>]*>[\s\S]*?<\/div>/u.exec(html)?.[0] ?? "";

  test("it is a labelled group of two real buttons that expose their state", () => {
    expect(group).toContain('id="process-view"');
    expect(group).toContain('role="group"');
    expect(group).toContain('aria-labelledby="process-view-label"');
    expect(html).toContain('<span class="label" id="process-view-label">List</span>');
    const buttons = group.match(/<button\b[^>]*>[^<]*<\/button>/gu) ?? [];
    expect(buttons).toEqual([
      '<button type="button" data-process-view="cpu" aria-pressed="true">By CPU</button>',
      '<button type="button" data-process-view="memory" aria-pressed="false" aria-describedby="process-view-notice">By memory</button>',
    ]);
  });

  test("it is not a tablist, and not a second switch", () => {
    // ADR 0033: a tablist is for tabs. ADR 0032: exactly one switch
    // implementation, the restyled checkbox inside the settings dialog.
    expect(group).not.toMatch(/role="(?:tablist|tab|switch)"/u);
    expect(group).not.toContain("<input");
    const processPanel = /<section class="panel process-panel"[\s\S]*?<\/section>/u.exec(html)?.[0] ?? "";
    expect(processPanel).not.toBe("");
    expect(processPanel).not.toMatch(/role="(?:tablist|tab|switch)"/u);
    expect(processPanel).not.toContain('type="checkbox"');
    // Every rule that styles a checkbox as a switch is scoped to the dialog.
    const switchRules = styles.match(/^[^{}\n]*input\[type="checkbox"\][^{}\n]*(?=,|\s*\{)/gmu) ?? [];
    expect(switchRules.length).toBeGreaterThan(0);
    for (const selector of switchRules) expect(selector).toContain(".settings-dialog");
  });

  test("it shares the one pill-group rule set instead of adding a second", () => {
    expect(styles).toMatch(/\.history-graph-nav,\s*\.history-window-nav,\s*\.process-view-nav\s*\{[^}]*display:\s*inline-flex/u);
    expect(styles).toMatch(
      /\.history-graph-nav button\[aria-pressed="true"\],\s*\.history-window-nav button\[aria-pressed="true"\],\s*\.process-view-nav button\[aria-pressed="true"\]\s*\{/u,
    );
    // No stand-alone block restyles the process buttons' pressed state.
    expect(styles).not.toMatch(/(?:^|\})\s*\.process-view-nav button\[aria-pressed="true"\]\s*\{/u);
  });

  test("every new class that sets `display` restates `[hidden]`", () => {
    expect(styles).toMatch(/\.process-view-nav\[hidden\]\s*\{[^}]*display:\s*none/u);
    expect(styles).toMatch(/\.process-controls label,\s*\.process-view-field\s*\{[^}]*display:\s*grid/u);
    expect(styles).toMatch(/\.process-view-field\[hidden\]\s*\{[^}]*display:\s*none/u);
    // The notice is hidden by attribute; its own class must not set `display`.
    const notice = /\.process-view-notice\s*\{([^}]*)\}/u.exec(styles)?.[1] ?? "missing";
    expect(notice).not.toBe("missing");
    expect(notice).not.toMatch(/display\s*:/u);
  });

  test("the notice is real text announced politely, hidden until there is something to say", () => {
    expect(html).toMatch(/<p class="[^"]*\bprocess-view-notice\b[^"]*" id="process-view-notice" role="status" hidden><\/p>/u);
    expect(app).toContain('setHidden(elements.processViewNotice, shown.notice === null)');
  });

  test("an unavailable memory list keeps the button focusable", () => {
    expect(app).toContain('button.setAttribute("aria-disabled", String(!shown.memoryAvailable))');
    expect(app).toContain('if (button.getAttribute("aria-disabled") === "true") return;');
    expect(app).not.toMatch(/processViewButtons[\s\S]{0,400}\.disabled\s*=/u);
    expect(styles).toContain('.process-view-nav button[aria-disabled="true"]');
  });

  test("the choice is remembered with the dashboard's other view preferences", () => {
    expect(app).toContain('processView: "tinytop.processView"');
    expect(app).toContain(
      "state.processView = readStoredValue(STORAGE_KEYS.processView, DEFAULT_PROCESS_VIEW, PROCESS_VIEW_KEYS);",
    );
    expect(app).toContain("storeValue(STORAGE_KEYS.processView, view);");
    // One storage mechanism: nothing touches localStorage for it directly.
    expect(app).not.toMatch(/localStorage\.(?:get|set)Item\(STORAGE_KEYS\.processView/u);
  });

  test("the table has a sortable Swap column between RSS and GPU, and the detail dialog shows swap", () => {
    const head = /<thead>[\s\S]*?<\/thead>/u.exec(html)?.[0] ?? "";
    const columns = Array.from(head.matchAll(/<th\b[^>]*>(?:<button[^>]*>)?([^<]+)/gu), (match) => match[1]);
    expect(columns).toEqual(["PID", "Command", "CPU", "RAM", "RSS", "Swap", "GPU", "Details"]);
    expect(head).toContain('<th scope="col" class="memory-basis-cell"><button type="button" data-process-sort="swap">Swap</button></th>');
    expect(html).toContain('id="process-detail-swap"');
    expect(app).toContain("row.append(pid, command, cpu, memory, rss, swap, gpu, details);");
    expect(app).toContain("cell.colSpan = hasGpu ? 8 : 7;");
    expect(app).toContain("swap.textContent = formatSwapBytes(process.swapBytes);");
  });

  test("the by-memory view marks RSS and Swap as the two parts of the ranking", () => {
    expect((html.match(/class="memory-basis-cell"/gu) ?? []).length).toBe(2);
    expect(styles).toContain('.process-panel table[data-process-view="memory"] th.memory-basis-cell');
    expect(app).toContain("elements.processTable.dataset.processView = shown.view;");
    expect(app).toContain("setText(elements.processViewDescription, PROCESS_VIEW_DESCRIPTIONS[shown.view]);");
  });

  test("the table renders the shown list and counts it", () => {
    expect(app).toContain("const shown = processViewFor(processes, state.processView);");
    expect(app).toContain("const visible = filteredProcesses(sortProcesses(shown.rows));");
    expect(app).toContain("setText(elements.processCount, processCounterText(visible.length, shown.rows.length));");
    expect(app).not.toContain("${processes.length} rows");
  });

  // A process command line comes from /proc: whoever can start a process on
  // the host chooses its text. It reaches the page through textContent and
  // title only. The script's two innerHTML assignments are the pause button's
  // constant icon templates; a third one is a place where that text, or any
  // other collected value, could be parsed as markup.
  test("collected text never reaches the page as markup", () => {
    const assignments = app.match(/[\w.]+\.(?:innerHTML|outerHTML)\s*\+?=/g) ?? [];
    expect(assignments).toEqual(["elements.pauseButton.innerHTML =", "elements.pauseButton.innerHTML ="]);
    expect(app).not.toContain("insertAdjacentHTML");
    expect(app).not.toContain("document.write");
    expect(app).toContain("command.textContent = process.command;");
  });
});
