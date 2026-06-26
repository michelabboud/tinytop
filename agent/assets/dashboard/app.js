const POLL_MS = 1500;
const MAX_HISTORY_PAGE_SIZE = 10_000;
const MAX_HISTORY_PAGE_COUNT = 8;
const MAX_HISTORY_RENDER_SAMPLES = 1_200;
const MAX_VISIBLE_HISTORY = 80;
const MIN_VISIBLE_BAR_SAMPLES = 12;
const MIN_STACKED_BAR_WIDTH = 12;
const BAR_CHART_SIDE_PADDING = 70;
const STORAGE_KEYS = {
  theme: "tinytop.theme",
  graphMode: "tinytop.graphMode",
  historyWindow: "tinytop.historyWindow",
};
const THEMES = new Set(["midnight", "matrix", "aurora", "solar", "ember"]);
const GRAPH_MODES = {
  line: "Line graph",
  area: "Area graph",
  bar: "Bar graph",
  heatmap: "Heatmap",
  treemap: "Treemap",
};
const HISTORY_METRICS = [
  { key: "cpu", label: "CPU" },
  { key: "ram", label: "RAM" },
  { key: "swap", label: "SWAP" },
  { key: "load", label: "LOAD" },
];
const HISTORY_WINDOWS = {
  live: { label: "Live", durationMs: 5 * 60 * 1000, pageSize: 240 },
  "15m": { label: "15m", durationMs: 15 * 60 * 1000, pageSize: 900 },
  "1h": { label: "1h", durationMs: 60 * 60 * 1000, pageSize: 2_400 },
  "6h": { label: "6h", durationMs: 6 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE },
  "24h": { label: "24h", durationMs: 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE },
};
const HISTORY_WINDOW_KEYS = new Set(Object.keys(HISTORY_WINDOWS));

const state = {
  paused: false,
  loading: false,
  timer: null,
  activeConfirmation: null,
  confirmationReturnFocus: null,
  historyChartInstance: null,
  theme: "midnight",
  graphMode: "line",
  historyWindowKey: "live",
  historyFetchToken: 0,
  snapshots: [],
  selectedAtMs: null,
  history: {
    cpu: [],
    ram: [],
    swap: [],
    load: [],
  },
};

const elements = {
  statusMessage: document.querySelector("#status-message"),
  liveDot: document.querySelector("#live-dot"),
  liveLabel: document.querySelector("#live-label"),
  runtimeSummary: document.querySelector("#runtime-summary"),
  hostName: document.querySelector("#host-name"),
  kernelName: document.querySelector("#kernel-name"),
  distroName: document.querySelector("#distro-name"),
  uptime: document.querySelector("#uptime"),
  cpuCores: document.querySelector("#cpu-cores"),
  cpuGauge: document.querySelector("#cpu-gauge"),
  cpuValue: document.querySelector("#cpu-value"),
  cpuSpark: document.querySelector("#cpu-spark"),
  ramGauge: document.querySelector("#ram-gauge"),
  ramValue: document.querySelector("#ram-value"),
  ramTotal: document.querySelector("#ram-total"),
  ramSpark: document.querySelector("#ram-spark"),
  swapGauge: document.querySelector("#swap-gauge"),
  swapValue: document.querySelector("#swap-value"),
  swapTotal: document.querySelector("#swap-total"),
  swapSpark: document.querySelector("#swap-spark"),
  loadOne: document.querySelector("#load-one"),
  loadContext: document.querySelector("#load-context"),
  threadCount: document.querySelector("#thread-count"),
  runnableCount: document.querySelector("#runnable-count"),
  rootUsed: document.querySelector("#root-used"),
  rootMount: document.querySelector("#root-mount"),
  runtimeKind: document.querySelector("#runtime-kind"),
  runtimeConfidence: document.querySelector("#runtime-confidence"),
  filesystemCount: document.querySelector("#filesystem-count"),
  filesystemList: document.querySelector("#filesystem-list"),
  pressureList: document.querySelector("#pressure-list"),
  historyChart: document.querySelector("#history-chart"),
  sampleCount: document.querySelector("#sample-count"),
  processCount: document.querySelector("#process-count"),
  processRows: document.querySelector("#process-rows"),
  refreshButton: document.querySelector("#refresh-button"),
  pauseButton: document.querySelector("#pause-button"),
  themeButtons: Array.from(document.querySelectorAll("[data-theme-option]")),
  graphButtons: Array.from(document.querySelectorAll("[data-graph-mode]")),
  historyWindowButtons: Array.from(document.querySelectorAll("[data-history-window]")),
  historyScrubber: document.querySelector("#history-scrubber"),
  historyPositionLabel: document.querySelector("#history-position-label"),
  historyStartLabel: document.querySelector("#history-start-label"),
  historyEndLabel: document.querySelector("#history-end-label"),
  historySampleValues: document.querySelector("#history-sample-values"),
  liveButton: document.querySelector("#live-button"),
  clearSessionButton: document.querySelector("#clear-session-button"),
  confirmationDialog: document.querySelector("#confirmation-dialog"),
  confirmationTitle: document.querySelector("#confirmation-title"),
  confirmationMessage: document.querySelector("#confirmation-message"),
  confirmationCancelButton: document.querySelector("#confirmation-cancel"),
  confirmationConfirmButton: document.querySelector("#confirmation-confirm"),
};

function readStoredValue(key, fallback, allowed) {
  try {
    const value = window.localStorage.getItem(key);
    return value && allowed.has(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

function storeValue(key, value) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // localStorage can be disabled; the controls should still work for the current session.
  }
}

function syncPressed(buttons, dataKey, activeValue) {
  for (const button of buttons) {
    button.setAttribute("aria-pressed", String(button.dataset[dataKey] === activeValue));
  }
}

function applyTheme(theme) {
  const nextTheme = THEMES.has(theme) ? theme : "midnight";
  state.theme = nextTheme;
  document.body.dataset.theme = nextTheme;
  syncPressed(elements.themeButtons, "themeOption", nextTheme);
  storeValue(STORAGE_KEYS.theme, nextTheme);
  redrawCharts();
}

function setGraphMode(mode) {
  const nextMode = Object.hasOwn(GRAPH_MODES, mode) ? mode : "line";
  state.graphMode = nextMode;
  syncPressed(elements.graphButtons, "graphMode", nextMode);
  storeValue(STORAGE_KEYS.graphMode, nextMode);
  drawHistoryChart();
}

function clampPercent(value) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function formatPercent(value) {
  return `${clampPercent(value).toFixed(value % 1 === 0 ? 0 : 1)}%`;
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function formatUptime(seconds) {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

function formatSampleTime(timestamp) {
  return new Date(timestamp).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatSampleDateTime(timestamp) {
  return new Date(timestamp).toLocaleString([], {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function setText(node, value) {
  if (node) node.textContent = value;
}

function setHidden(node, hidden) {
  if (node) node.hidden = hidden;
}

function setGauge(node, value) {
  node?.style.setProperty("--value", String(clampPercent(value)));
}

function closeConfirmationDialog(accepted) {
  const resolver = state.activeConfirmation;
  state.activeConfirmation = null;

  if (elements.confirmationDialog?.open) {
    elements.confirmationDialog.close();
  } else {
    elements.confirmationDialog?.removeAttribute("open");
  }

  if (state.confirmationReturnFocus instanceof HTMLElement) {
    state.confirmationReturnFocus.focus();
  }
  state.confirmationReturnFocus = null;

  resolver?.(accepted);
}

function requestConfirmation({ title, message, confirmLabel = "Confirm", cancelLabel = "Cancel", tone = "default" }) {
  if (!elements.confirmationDialog) return Promise.resolve(false);

  if (state.activeConfirmation) closeConfirmationDialog(false);

  setText(elements.confirmationTitle, title);
  setText(elements.confirmationMessage, message);
  setText(elements.confirmationConfirmButton, confirmLabel);
  setText(elements.confirmationCancelButton, cancelLabel);
  elements.confirmationConfirmButton?.classList.toggle("danger", tone === "danger");
  state.confirmationReturnFocus = document.activeElement;

  return new Promise((resolve) => {
    state.activeConfirmation = resolve;
    if (typeof elements.confirmationDialog.showModal === "function") {
      elements.confirmationDialog.showModal();
    } else {
      elements.confirmationDialog.setAttribute("open", "");
    }
    elements.confirmationCancelButton?.focus();
  });
}

function cssColor(name) {
  return getComputedStyle(document.body).getPropertyValue(name).trim();
}

function chartPalette() {
  return {
    cpu: cssColor("--chart-cpu") || "#22c55e",
    ram: cssColor("--chart-ram") || "#38bdf8",
    swap: cssColor("--chart-swap") || "#a78bfa",
    load: cssColor("--chart-load") || "#f59e0b",
    grid: cssColor("--border") || "#263244",
    text: cssColor("--text") || "#f8fafc",
    muted: cssColor("--muted") || "#94a3b8",
  };
}

function historySeries() {
  const palette = chartPalette();
  return [
    { label: "CPU", values: state.history.cpu, color: palette.cpu, dashed: false },
    { label: "RAM", values: state.history.ram, color: palette.ram, dashed: false },
    { label: "SWAP", values: state.history.swap, color: palette.swap, dashed: false },
    { label: "LOAD", values: state.history.load, color: palette.load, dashed: true },
  ];
}

function redrawCharts() {
  const palette = chartPalette();
  drawSparkline(elements.cpuSpark, state.history.cpu, palette.cpu);
  drawSparkline(elements.ramSpark, state.history.ram, palette.ram);
  drawSparkline(elements.swapSpark, state.history.swap, palette.swap);
  drawHistoryChart();
}

function snapshotCapturedAtMs(snapshot) {
  const parsed = Date.parse(snapshot.timestamp);
  return Number.isFinite(parsed) ? parsed : Date.now();
}

function snapshotMetricValues(snapshot) {
  return {
    cpu: snapshot.cpu.usagePercent,
    ram: snapshot.memory.usedPercent,
    swap: snapshot.swap.usedPercent,
    load: Math.min(100, (snapshot.load.one / Math.max(1, snapshot.cpu.cores)) * 100),
  };
}

function setHistoryValues(index, snapshot) {
  const values = snapshotMetricValues(snapshot);
  state.history.cpu[index] = values.cpu;
  state.history.ram[index] = values.ram;
  state.history.swap[index] = values.swap;
  state.history.load[index] = values.load;
}

function rebuildHistoryValues() {
  state.history.cpu = [];
  state.history.ram = [];
  state.history.swap = [];
  state.history.load = [];
  state.snapshots.forEach((sample, index) => {
    setHistoryValues(index, sample.snapshot);
  });
}

function resetHistory({ keepSelection = false } = {}) {
  state.snapshots = [];
  state.history.cpu = [];
  state.history.ram = [];
  state.history.swap = [];
  state.history.load = [];
  if (!keepSelection) state.selectedAtMs = null;
}

function trimHistory() {
  if (state.snapshots.length > MAX_HISTORY_RENDER_SAMPLES) {
    state.snapshots = state.snapshots.slice(-MAX_HISTORY_RENDER_SAMPLES);
    rebuildHistoryValues();
  }
}

function pushHistory(snapshot, capturedAtMs = snapshotCapturedAtMs(snapshot)) {
  const capturedAt = Number.isFinite(Number(capturedAtMs)) ? Number(capturedAtMs) : snapshotCapturedAtMs(snapshot);
  const existingIndex = state.snapshots.findIndex((sample) => sample.capturedAt === capturedAt);

  if (existingIndex !== -1) {
    state.snapshots[existingIndex] = { capturedAt, snapshot };
    state.snapshots.sort((left, right) => left.capturedAt - right.capturedAt);
    rebuildHistoryValues();
    return;
  }

  state.snapshots.push({ capturedAt, snapshot });
  state.snapshots.sort((left, right) => left.capturedAt - right.capturedAt);
  rebuildHistoryValues();
  trimHistory();
}

function normalizedHistorySamples(samples) {
  const byTimestamp = new Map();
  if (!Array.isArray(samples)) return [];

  for (const sample of samples) {
    if (!sample?.snapshot) continue;
    const capturedAt = Number.isFinite(Number(sample.capturedAtMs))
      ? Number(sample.capturedAtMs)
      : snapshotCapturedAtMs(sample.snapshot);
    byTimestamp.set(capturedAt, { capturedAt, snapshot: sample.snapshot });
  }

  return Array.from(byTimestamp.values()).sort((left, right) => left.capturedAt - right.capturedAt);
}

function downsampleHistorySamples(samples, maxSamples) {
  if (samples.length <= maxSamples) return samples;
  if (maxSamples <= 1) return samples.slice(-1);

  const selected = [];
  const seen = new Set();
  const step = (samples.length - 1) / (maxSamples - 1);

  for (let index = 0; index < maxSamples; index += 1) {
    const sample = samples[Math.round(index * step)];
    if (!sample || seen.has(sample.capturedAt)) continue;
    selected.push(sample);
    seen.add(sample.capturedAt);
  }

  return selected;
}

function hydrateHistory(samples, { keepSelection = false } = {}) {
  const selectedAtMs = state.selectedAtMs;
  resetHistory({ keepSelection });
  state.snapshots = downsampleHistorySamples(normalizedHistorySamples(samples), MAX_HISTORY_RENDER_SAMPLES);
  if (keepSelection) state.selectedAtMs = selectedAtMs;
  if (state.snapshots.length === 0) state.selectedAtMs = null;
  rebuildHistoryValues();

  renderSelectedSample();
  updateHistoryControls();
  redrawCharts();
}

function drawSparkline(canvas, values, color) {
  if (!canvas) return;
  const context = canvas.getContext("2d");
  const palette = chartPalette();
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth || canvas.width;
  const height = canvas.clientHeight || canvas.height;
  canvas.width = Math.floor(width * ratio);
  canvas.height = Math.floor(height * ratio);
  context.scale(ratio, ratio);
  context.clearRect(0, 0, width, height);
  context.strokeStyle = palette.grid;
  context.globalAlpha = 0.45;
  context.lineWidth = 1;
  for (let line = 1; line < 3; line += 1) {
    const y = (height / 3) * line;
    context.beginPath();
    context.moveTo(0, y);
    context.lineTo(width, y);
    context.stroke();
  }
  context.globalAlpha = 1;
  if (values.length < 2) return;
  context.strokeStyle = color;
  context.lineWidth = 2;
  context.beginPath();
  const denominator = Math.max(1, values.length - 1);
  values.forEach((value, index) => {
    const x = (index / denominator) * width;
    const y = height - (clampPercent(value) / 100) * height;
    if (index === 0) context.moveTo(x, y);
    else context.lineTo(x, y);
  });
  context.stroke();
}

function visibleHistoryRange() {
  const total = state.snapshots.length;
  if (total === 0) return { start: 0, end: 0, total, visible: 0 };

  const visible = Math.min(total, visibleHistoryCapacity());
  if (total <= visible) return { start: 0, end: total, total, visible };

  const liveStart = total - visible;
  const selectedIndex = inspectedIndex();
  if (selectedIndex === null) {
    return { start: liveStart, end: total, total, visible };
  }

  const centeredStart = selectedIndex - Math.floor(visible / 2);
  const start = Math.max(0, Math.min(centeredStart, liveStart));
  return { start, end: start + visible, total, visible };
}

function visibleHistoryCapacity() {
  if (state.graphMode !== "bar") return MAX_VISIBLE_HISTORY;
  const width = elements.historyChart?.clientWidth ?? 0;
  if (width <= 0) return MAX_VISIBLE_HISTORY;
  const plotWidth = Math.max(MIN_STACKED_BAR_WIDTH, width - BAR_CHART_SIDE_PADDING);
  const capacity = Math.floor(plotWidth / MIN_STACKED_BAR_WIDTH);
  return Math.max(MIN_VISIBLE_BAR_SAMPLES, Math.min(MAX_HISTORY_RENDER_SAMPLES, capacity));
}

function visibleHistorySeries(series, range) {
  return series.map((item) => ({
    ...item,
    values: item.values.slice(range.start, range.end),
  }));
}

function echartsApi() {
  return window.echarts;
}

function historyChartInstance() {
  const echarts = echartsApi();
  if (!elements.historyChart || !echarts) return null;
  if (!state.historyChartInstance) {
    state.historyChartInstance = echarts.init(elements.historyChart, null, { renderer: "canvas" });
    state.historyChartInstance.on("click", handleHistoryChartClick);
    state.historyChartInstance.getZr().on("click", handleHistoryPlotClick);
  }
  return state.historyChartInstance;
}

function historyChartColors(palette) {
  return [palette.cpu, palette.ram, palette.swap, palette.load];
}

function historyCategories(range) {
  return state.snapshots.slice(range.start, range.end).map((sample) => formatSampleTime(sample.capturedAt));
}

function baseCartesianOption(palette, range) {
  return {
    animation: false,
    color: historyChartColors(palette),
    backgroundColor: "transparent",
    grid: {
      top: 42,
      right: 18,
      bottom: 28,
      left: 52,
      containLabel: false,
    },
    legend: {
      top: 8,
      left: 8,
      itemWidth: 16,
      itemHeight: 3,
      textStyle: { color: palette.text },
      data: HISTORY_METRICS.map((metric) => metric.label),
    },
    tooltip: {
      trigger: "axis",
      confine: true,
      valueFormatter: (value) => formatPercent(Number(value)),
      axisPointer: { type: state.graphMode === "bar" ? "shadow" : "line" },
    },
    xAxis: {
      type: "category",
      boundaryGap: state.graphMode === "bar",
      data: historyCategories(range),
      axisLine: { lineStyle: { color: palette.grid } },
      axisTick: { show: false },
      axisLabel: {
        color: palette.muted,
        hideOverlap: true,
        maxInterval: Math.max(1, Math.floor(Math.max(1, range.visible) / 6)),
      },
    },
    yAxis: {
      type: "value",
      min: 0,
      axisLabel: {
        color: palette.muted,
        formatter: "{value}%",
      },
      splitLine: { lineStyle: { color: palette.grid } },
    },
  };
}

function lineSeries(series) {
  return series.map((item) => ({
    name: item.label,
    type: "line",
    data: item.values.map(clampPercent),
    showSymbol: false,
    symbol: "circle",
    symbolSize: 5,
    smooth: false,
    lineStyle: {
      width: item.dashed ? 2 : 2.5,
      type: item.dashed ? "dashed" : "solid",
    },
    emphasis: { focus: "series" },
  }));
}

function areaSeries(series) {
  return series.map((item) => ({
    name: item.label,
    type: "line",
    stack: "total",
    data: item.values.map(clampPercent),
    showSymbol: false,
    smooth: false,
    areaStyle: { opacity: item.label === "LOAD" ? 0.42 : 0.55 },
    lineStyle: { width: 1.8 },
    emphasis: { focus: "series" },
  }));
}

function barSeries(series) {
  return series.map((item) => ({
    name: item.label,
    type: "bar",
    stack: "total",
    data: item.values.map(clampPercent),
    barMinWidth: MIN_STACKED_BAR_WIDTH,
    barMaxWidth: 28,
    emphasis: { focus: "series" },
  }));
}

function buildLineOption(palette, range, series) {
  return {
    ...baseCartesianOption(palette, range),
    yAxis: {
      ...baseCartesianOption(palette, range).yAxis,
      max: 100,
    },
    series: lineSeries(series),
  };
}

function buildAreaOption(palette, range, series) {
  return {
    ...baseCartesianOption(palette, range),
    tooltip: {
      ...baseCartesianOption(palette, range).tooltip,
      order: "seriesDesc",
    },
    series: areaSeries(series),
  };
}

function buildBarOption(palette, range, series) {
  return {
    ...baseCartesianOption(palette, range),
    series: barSeries(series),
  };
}

function buildHeatmapOption(palette, range, series) {
  const categories = historyCategories(range);
  const data = [];
  series.forEach((item, rowIndex) => {
    item.values.forEach((value, index) => {
      data.push([index, rowIndex, clampPercent(value)]);
    });
  });

  return {
    animation: false,
    backgroundColor: "transparent",
    grid: {
      top: 36,
      right: 18,
      bottom: 46,
      left: 54,
      containLabel: false,
    },
    tooltip: {
      position: "top",
      confine: true,
      formatter: (params) => {
        const [sampleIndex, metricIndex, value] = params.value;
        return `${series[metricIndex]?.label ?? "Metric"}<br>${categories[sampleIndex]}: ${formatPercent(value)}`;
      },
    },
    xAxis: {
      type: "category",
      data: categories,
      splitArea: { show: true },
      axisTick: { show: false },
      axisLine: { lineStyle: { color: palette.grid } },
      axisLabel: {
        color: palette.muted,
        hideOverlap: true,
        maxInterval: Math.max(1, Math.floor(Math.max(1, range.visible) / 6)),
      },
    },
    yAxis: {
      type: "category",
      data: series.map((item) => item.label),
      axisTick: { show: false },
      axisLine: { lineStyle: { color: palette.grid } },
      axisLabel: { color: palette.text },
    },
    visualMap: {
      min: 0,
      max: 100,
      calculable: false,
      orient: "horizontal",
      left: "center",
      bottom: 8,
      textStyle: { color: palette.muted },
      inRange: {
        color: ["#e0f2fe", "#38bdf8", "#22c55e", "#f59e0b", "#ef4444"],
      },
    },
    series: [
      {
        name: "Usage",
        type: "heatmap",
        data,
        label: { show: false },
        emphasis: {
          itemStyle: {
            shadowBlur: 8,
            shadowColor: "rgba(0, 0, 0, 0.22)",
          },
        },
      },
    ],
  };
}

function buildTreemapOption(palette) {
  const sample = selectedSample();
  const colors = historyChartColors(palette);
  const data = sampleMetricValues(sample).map(([name, value], index) => ({
    name,
    value: Math.max(0.1, clampPercent(value)),
    itemStyle: { color: colors[index] },
    label: {
      formatter: `${name}\n${formatPercent(value)}`,
    },
  }));

  return {
    animation: false,
    backgroundColor: "transparent",
    tooltip: {
      confine: true,
      formatter: (params) => `${params.name}: ${formatPercent(Number(params.value))}`,
    },
    series: [
      {
        type: "treemap",
        roam: false,
        nodeClick: false,
        breadcrumb: { show: false },
        top: 16,
        bottom: 12,
        left: 12,
        right: 12,
        label: {
          show: true,
          color: palette.text,
          fontWeight: 800,
          fontSize: 13,
        },
        upperLabel: { show: false },
        itemStyle: {
          borderColor: palette.grid,
          borderWidth: 2,
          gapWidth: 4,
        },
        data,
      },
    ],
  };
}

function historyChartOption(palette, range, series) {
  if (state.graphMode === "area") return buildAreaOption(palette, range, series);
  if (state.graphMode === "bar") return buildBarOption(palette, range, series);
  if (state.graphMode === "heatmap") return buildHeatmapOption(palette, range, series);
  if (state.graphMode === "treemap") return buildTreemapOption(palette);
  return buildLineOption(palette, range, series);
}

function drawHistoryChart() {
  const chart = historyChartInstance();
  if (!chart) return;
  const palette = chartPalette();
  const range = visibleHistoryRange();
  const series = visibleHistorySeries(historySeries(), range);
  chart.setOption(historyChartOption(palette, range, series), true);
}

function nearestHistoryIndex(timestampMs) {
  if (state.snapshots.length === 0) return null;
  const target = Number(timestampMs);
  if (!Number.isFinite(target)) return state.snapshots.length - 1;

  let bestIndex = 0;
  let bestDistance = Math.abs(state.snapshots[0].capturedAt - target);
  for (let index = 1; index < state.snapshots.length; index += 1) {
    const distance = Math.abs(state.snapshots[index].capturedAt - target);
    if (distance < bestDistance) {
      bestIndex = index;
      bestDistance = distance;
    }
  }
  return bestIndex;
}

function inspectedIndex() {
  if (state.selectedAtMs === null) return null;
  return nearestHistoryIndex(state.selectedAtMs);
}

function activeHistoryIndex() {
  if (state.snapshots.length === 0) return null;
  return inspectedIndex() ?? state.snapshots.length - 1;
}

function selectedSample() {
  if (state.snapshots.length === 0) return null;
  const index = activeHistoryIndex();
  return index === null ? null : state.snapshots[index];
}

function sampleMetricValues(sample) {
  if (!sample) return [];
  const snapshot = sample.snapshot;
  const loadPercent = Math.min(100, (snapshot.load.one / Math.max(1, snapshot.cpu.cores)) * 100);
  return [
    ["CPU", snapshot.cpu.usagePercent],
    ["RAM", snapshot.memory.usedPercent],
    ["SWAP", snapshot.swap.usedPercent],
    ["LOAD", loadPercent],
  ];
}

function renderHistorySampleValues(sample) {
  if (!elements.historySampleValues) return;
  const metrics = sampleMetricValues(sample);
  elements.historySampleValues.replaceChildren(
    ...metrics.map(([name, value]) => {
      const item = document.createElement("span");
      item.className = "history-sample-value";
      const label = document.createElement("span");
      const strong = document.createElement("strong");
      label.textContent = name;
      strong.textContent = formatPercent(value);
      item.append(label, strong);
      return item;
    }),
  );
}

function historySampleCountText(sampleCount, range) {
  const base = `${sampleCount} ${sampleCount === 1 ? "sample" : "samples"}`;
  if (sampleCount > range.visible && range.visible > 0) return `${base} / ${range.visible} shown`;
  return base;
}

function updateHistoryChartTitle(sample) {
  if (!elements.historyChart) return;
  if (!sample) {
    elements.historyChart.title = "No history samples loaded";
    return;
  }
  const metrics = sampleMetricValues(sample)
    .map(([name, value]) => `${name} ${formatPercent(value)}`)
    .join(", ");
  elements.historyChart.title = `${formatSampleDateTime(sample.capturedAt)} - ${metrics}`;
}

function updateHistoryControls() {
  const sampleCount = state.snapshots.length;
  const lastIndex = Math.max(0, sampleCount - 1);
  const activeIndex = activeHistoryIndex() ?? lastIndex;
  const activeSample = selectedSample();
  const firstSample = state.snapshots[0];
  const latestSample = state.snapshots[lastIndex];
  const range = visibleHistoryRange();
  const isLive = state.selectedAtMs === null;
  const windowLabel = HISTORY_WINDOWS[state.historyWindowKey]?.label ?? "Live";
  const positionText =
    activeSample && isLive
      ? `${windowLabel} - ${formatSampleDateTime(activeSample.capturedAt)}`
      : activeSample
        ? `Viewing ${formatSampleDateTime(activeSample.capturedAt)}`
        : "Live";
  const ariaValueText =
    activeSample && isLive
      ? `${windowLabel}, latest sample ${formatSampleDateTime(activeSample.capturedAt)}`
      : activeSample
        ? `Sample ${activeIndex + 1} of ${sampleCount}, ${formatSampleDateTime(activeSample.capturedAt)}`
        : "Live";

  setText(elements.sampleCount, historySampleCountText(sampleCount, range));
  setText(elements.historyPositionLabel, positionText);
  setText(elements.historyStartLabel, firstSample ? formatSampleDateTime(firstSample.capturedAt) : "-");
  setText(
    elements.historyEndLabel,
    latestSample ? `${windowLabel} - ${formatSampleTime(latestSample.capturedAt)}` : windowLabel,
  );
  renderHistorySampleValues(activeSample);
  updateHistoryChartTitle(activeSample);

  if (elements.historyScrubber) {
    elements.historyScrubber.disabled = !firstSample || !latestSample || firstSample.capturedAt === latestSample.capturedAt;
    elements.historyScrubber.min = String(firstSample?.capturedAt ?? 0);
    elements.historyScrubber.max = String(latestSample?.capturedAt ?? 0);
    elements.historyScrubber.value = String(activeSample?.capturedAt ?? latestSample?.capturedAt ?? 0);
    elements.historyScrubber.setAttribute("aria-valuetext", ariaValueText);
  }

  if (elements.historyChart) {
    elements.historyChart.setAttribute("aria-valuemin", String(firstSample?.capturedAt ?? 0));
    elements.historyChart.setAttribute("aria-valuemax", String(latestSample?.capturedAt ?? 0));
    elements.historyChart.setAttribute("aria-valuenow", String(activeSample?.capturedAt ?? latestSample?.capturedAt ?? 0));
    elements.historyChart.setAttribute("aria-valuetext", ariaValueText);
  }

  if (elements.liveButton) {
    elements.liveButton.disabled = isLive;
  }

  if (elements.clearSessionButton) {
    elements.clearSessionButton.disabled = sampleCount === 0;
  }
}

function renderSelectedSample() {
  const sample = selectedSample();
  if (!sample) {
    updateHistoryControls();
    return;
  }
  renderSnapshotDetails(sample.snapshot);
  updateHistoryControls();
}

function selectHistoryTimestamp(timestampMs) {
  const index = nearestHistoryIndex(timestampMs);
  if (index === null) return;
  const lastIndex = state.snapshots.length - 1;
  if (index >= lastIndex) {
    returnToLiveHistory();
    return;
  }
  state.selectedAtMs = state.snapshots[index].capturedAt;
  renderSelectedSample();
  redrawCharts();
}

function returnToLiveHistory() {
  state.selectedAtMs = null;
  renderSelectedSample();
  redrawCharts();
}

function clearSessionHistory() {
  resetHistory();
  updateHistoryControls();
  redrawCharts();
}

function selectHistoryPosition(index) {
  if (state.snapshots.length === 0) return;
  const lastIndex = state.snapshots.length - 1;
  const nextIndex = Math.max(0, Math.min(index, lastIndex));
  if (nextIndex >= lastIndex) returnToLiveHistory();
  else selectHistoryTimestamp(state.snapshots[nextIndex].capturedAt);
}

function historySampleIndexFromChartParams(params) {
  const range = visibleHistoryRange();
  if (range.visible <= 0) return null;
  if (state.graphMode === "treemap") return activeHistoryIndex() ?? state.snapshots.length - 1;

  const rawIndex = Array.isArray(params.value) ? params.value[0] : params.dataIndex;
  const visibleIndex = Number(rawIndex);
  if (!Number.isFinite(visibleIndex)) return null;
  return Math.max(range.start, Math.min(range.start + visibleIndex, range.end - 1));
}

function handleHistoryChartClick(params) {
  const index = historySampleIndexFromChartParams(params);
  if (index === null) return;
  selectHistoryPosition(index);
}

function handleHistoryPlotClick(event) {
  const chart = state.historyChartInstance;
  if (!chart || state.graphMode === "treemap") return;

  const point = [event.offsetX, event.offsetY];
  if (!chart.containPixel({ gridIndex: 0 }, point)) return;

  const range = visibleHistoryRange();
  const visibleCount = range.end - range.start;
  if (visibleCount <= 0) return;

  const [rawIndex] = chart.convertFromPixel({ gridIndex: 0 }, point);
  const visibleIndex =
    state.graphMode === "bar" || state.graphMode === "heatmap"
      ? Math.floor(Number(rawIndex))
      : Math.round(Number(rawIndex));
  if (!Number.isFinite(visibleIndex)) return;

  selectHistoryPosition(Math.max(range.start, Math.min(range.start + visibleIndex, range.end - 1)));
}

function moveHistorySelection(delta) {
  if (state.snapshots.length === 0) return;
  const lastIndex = state.snapshots.length - 1;
  const currentIndex = activeHistoryIndex() ?? lastIndex;
  selectHistoryPosition(currentIndex + delta);
}

function renderFilesystems(filesystems) {
  setText(elements.filesystemCount, `${filesystems.length} mounts`);
  elements.filesystemList.replaceChildren(
    ...filesystems.map((fs) => {
      const item = document.createElement("div");
      item.className = "filesystem-item";
      const inodeValue = fs.inodeUsedPercent ?? 0;
      item.innerHTML = `
        <div class="fs-name">
          <strong title="${fs.mount}">${fs.mount}</strong>
          <span>${fs.filesystem} - ${fs.type} - ${formatBytes(fs.usedBytes)} / ${formatBytes(fs.sizeBytes)}</span>
        </div>
        <div class="bar-group">
          <div>
            <div class="bar-label"><span>Capacity</span><span>${formatPercent(fs.usedPercent)}</span></div>
            <div class="bar ${fs.usedPercent >= 85 ? "warn" : ""}" style="--value:${clampPercent(fs.usedPercent)}"><span></span></div>
          </div>
          <div>
            <div class="bar-label"><span>Inodes</span><span>${fs.inodeUsedPercent === null ? "n/a" : formatPercent(inodeValue)}</span></div>
            <div class="bar ${inodeValue >= 85 ? "warn" : ""}" style="--value:${clampPercent(inodeValue)}"><span></span></div>
          </div>
        </div>
      `;
      return item;
    }),
  );
}

function pressureValue(snapshot, key) {
  return snapshot.pressure[key]?.some?.avg10 ?? 0;
}

function renderPressure(snapshot) {
  const items = [
    ["CPU", pressureValue(snapshot, "cpu"), "Scheduler contention"],
    ["Memory", pressureValue(snapshot, "memory"), "Allocation stalls"],
    ["I/O", pressureValue(snapshot, "io"), "Storage wait"],
  ];
  elements.pressureList.replaceChildren(
    ...items.map(([name, value, description]) => {
      const item = document.createElement("div");
      item.className = "pressure-item";
      item.innerHTML = `
        <div class="pressure-top">
          <strong>${name}</strong>
          <span>${Number(value).toFixed(2)} avg10</span>
        </div>
        <div class="bar ${value >= 10 ? "warn" : ""}" style="--value:${clampPercent(value * 5)}"><span></span></div>
        <span class="label">${description}</span>
      `;
      return item;
    }),
  );
}

function renderProcesses(processes) {
  setText(elements.processCount, `${processes.length} rows`);
  elements.processRows.replaceChildren(
    ...processes.map((process) => {
      const row = document.createElement("tr");
      row.innerHTML = `
        <td>${process.pid}</td>
        <td class="command-cell" title="${process.command}">${process.command}</td>
        <td>${process.cpuPercent.toFixed(1)}%</td>
        <td>${process.memoryPercent.toFixed(1)}%</td>
        <td>${formatBytes(process.rssBytes)}</td>
      `;
      return row;
    }),
  );
}

function renderSnapshotDetails(snapshot) {
  const rootFs = snapshot.filesystems.find((fs) => fs.mount === "/") ?? snapshot.filesystems[0];

  setText(elements.hostName, snapshot.identity.hostname);
  setText(elements.kernelName, snapshot.identity.kernel);
  setText(elements.distroName, snapshot.identity.distro);
  setText(elements.uptime, formatUptime(snapshot.identity.uptimeSeconds));
  setText(elements.runtimeSummary, `${snapshot.identity.runtime.kind} - ${snapshot.identity.runtime.reason}`);
  setText(elements.runtimeKind, snapshot.identity.runtime.kind);
  setText(elements.runtimeConfidence, `${snapshot.identity.runtime.confidence} confidence`);

  setText(elements.cpuValue, formatPercent(snapshot.cpu.usagePercent));
  setText(elements.cpuCores, `${snapshot.cpu.cores} cores`);
  setGauge(elements.cpuGauge, snapshot.cpu.usagePercent);

  setText(elements.ramValue, formatPercent(snapshot.memory.usedPercent));
  setText(elements.ramTotal, formatBytes(snapshot.memory.totalBytes));
  setGauge(elements.ramGauge, snapshot.memory.usedPercent);

  setText(elements.swapValue, formatPercent(snapshot.swap.usedPercent));
  setText(elements.swapTotal, formatBytes(snapshot.swap.totalBytes));
  setGauge(elements.swapGauge, snapshot.swap.usedPercent);

  setText(elements.loadOne, snapshot.load.one.toFixed(2));
  setText(elements.loadContext, `${snapshot.load.five.toFixed(2)} / ${snapshot.load.fifteen.toFixed(2)} 5m/15m`);
  setText(elements.threadCount, String(snapshot.load.totalThreads));
  setText(elements.runnableCount, `${snapshot.load.runnable} runnable`);
  setText(elements.rootUsed, rootFs ? formatPercent(rootFs.usedPercent) : "-");
  setText(elements.rootMount, rootFs ? `${rootFs.mount} on ${rootFs.filesystem}` : "-");

  renderFilesystems(snapshot.filesystems.slice(0, 8));
  renderPressure(snapshot);
  renderProcesses(snapshot.processes);
}

function renderSnapshot(snapshot) {
  pushHistory(snapshot);
  renderSelectedSample();
  redrawCharts();
}

function setLiveStatus(kind, label) {
  elements.liveDot.className = `status-dot ${kind}`;
  setText(elements.liveLabel, label);
}

async function fetchSnapshot() {
  if (state.loading || state.paused) return;
  state.loading = true;
  try {
    const response = await fetch("/api/snapshot", { cache: "no-store" });
    if (!response.ok) throw new Error(`Snapshot failed with HTTP ${response.status}`);
    const snapshot = await response.json();
    setHidden(elements.statusMessage, true);
    renderSnapshot(snapshot);
    setLiveStatus("live", "Live");
  } catch (error) {
    setHidden(elements.statusMessage, false);
    setText(elements.statusMessage, error instanceof Error ? error.message : "Snapshot failed");
    setLiveStatus("error", "Error");
  } finally {
    state.loading = false;
  }
}

async function fetchHistory() {
  return fetchHistoryWindow();
}

async function fetchHistoryPage({ sinceMs, untilMs, limit }) {
  const params = new URLSearchParams({
    limit: String(Math.min(MAX_HISTORY_PAGE_SIZE, Math.max(1, Math.floor(limit)))),
    since_ms: String(Math.floor(sinceMs)),
    until_ms: String(Math.floor(untilMs)),
  });
  const response = await fetch(`/api/history?${params}`, {
    cache: "no-store",
  });
  if (!response.ok) return [];
  const body = await response.json();
  return Array.isArray(body.samples) ? body.samples : [];
}

async function fetchHistoryWindow() {
  const fetchToken = state.historyFetchToken + 1;
  state.historyFetchToken = fetchToken;
  const windowConfig = HISTORY_WINDOWS[state.historyWindowKey] ?? HISTORY_WINDOWS.live;
  const untilMs = Date.now();
  const sinceMs = untilMs - windowConfig.durationMs;
  const limit = Math.min(MAX_HISTORY_PAGE_SIZE, windowConfig.pageSize);
  let pageUntilMs = untilMs;
  const samples = [];

  try {
    for (let page = 0; page < MAX_HISTORY_PAGE_COUNT; page += 1) {
      const pageSamples = await fetchHistoryPage({ sinceMs, untilMs: pageUntilMs, limit });
      if (state.historyFetchToken !== fetchToken) return;
      if (pageSamples.length === 0) break;

      samples.push(...pageSamples);
      const normalizedPage = normalizedHistorySamples(pageSamples);
      const oldestSample = normalizedPage[0];
      if (pageSamples.length < limit || !oldestSample || oldestSample.capturedAt <= sinceMs) break;
      pageUntilMs = oldestSample.capturedAt - 1;
    }

    hydrateHistory(samples, { keepSelection: state.selectedAtMs !== null });
  } catch {
    // Live polling still gives the dashboard useful data when history is unavailable.
  }
}

function setHistoryWindow(key, { fetch = true, persist = true } = {}) {
  const nextWindow = Object.hasOwn(HISTORY_WINDOWS, key) ? key : "live";
  state.historyWindowKey = nextWindow;
  state.selectedAtMs = null;
  syncPressed(elements.historyWindowButtons, "historyWindow", nextWindow);
  if (persist) storeValue(STORAGE_KEYS.historyWindow, nextWindow);
  updateHistoryControls();
  if (fetch) fetchHistoryWindow();
}

function setPaused(paused) {
  state.paused = paused;
  if (paused) {
    setLiveStatus("", "Paused");
    elements.pauseButton.innerHTML = `
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M8 5v14l11-7z" />
      </svg>
      <span>Resume</span>
    `;
    elements.pauseButton.title = "Resume live polling";
  } else {
    setLiveStatus("live", "Live");
    elements.pauseButton.innerHTML = `
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M8 5v14" />
        <path d="M16 5v14" />
      </svg>
      <span>Pause</span>
    `;
    elements.pauseButton.title = "Pause live polling";
    fetchSnapshot();
  }
}

elements.refreshButton.addEventListener("click", () => {
  const wasPaused = state.paused;
  state.paused = false;
  fetchSnapshot().finally(() => {
    state.paused = wasPaused;
    if (wasPaused) setLiveStatus("", "Paused");
  });
});

elements.pauseButton.addEventListener("click", () => {
  setPaused(!state.paused);
});

for (const button of elements.themeButtons) {
  button.addEventListener("click", () => {
    applyTheme(button.dataset.themeOption);
  });
}

for (const button of elements.graphButtons) {
  button.addEventListener("click", () => {
    setGraphMode(button.dataset.graphMode);
  });
}

for (const button of elements.historyWindowButtons) {
  button.addEventListener("click", () => {
    setHistoryWindow(button.dataset.historyWindow);
  });
}

elements.historyScrubber?.addEventListener("input", () => {
  selectHistoryTimestamp(Number(elements.historyScrubber.value));
});

elements.liveButton?.addEventListener("click", () => {
  returnToLiveHistory();
});

elements.clearSessionButton?.addEventListener("click", async () => {
  const accepted = await requestConfirmation({
    title: "Clear session history?",
    message:
      "This clears only the samples currently loaded in this browser tab. It does not delete the SQLite history database or change system data.",
    confirmLabel: "Clear session",
    tone: "danger",
  });
  if (accepted) clearSessionHistory();
});

elements.confirmationCancelButton?.addEventListener("click", () => {
  closeConfirmationDialog(false);
});

elements.confirmationConfirmButton?.addEventListener("click", () => {
  closeConfirmationDialog(true);
});

elements.confirmationDialog?.addEventListener("cancel", (event) => {
  event.preventDefault();
  closeConfirmationDialog(false);
});

elements.confirmationDialog?.addEventListener("click", (event) => {
  if (event.target === elements.confirmationDialog) closeConfirmationDialog(false);
});

elements.historyChart?.addEventListener("keydown", (event) => {
  if (event.key === "ArrowLeft") {
    event.preventDefault();
    moveHistorySelection(-1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    moveHistorySelection(1);
  } else if (event.key === "Home") {
    event.preventDefault();
    selectHistoryPosition(0);
  } else if (event.key === "End") {
    event.preventDefault();
    returnToLiveHistory();
  }
});

window.addEventListener("resize", () => {
  state.historyChartInstance?.resize();
  redrawCharts();
});

applyTheme(readStoredValue(STORAGE_KEYS.theme, "midnight", THEMES));
setGraphMode(readStoredValue(STORAGE_KEYS.graphMode, "line", new Set(Object.keys(GRAPH_MODES))));
setHistoryWindow(readStoredValue(STORAGE_KEYS.historyWindow, "live", HISTORY_WINDOW_KEYS), { fetch: false, persist: false });

async function startDashboard() {
  await fetchHistory();
  await fetchSnapshot();
  state.timer = window.setInterval(fetchSnapshot, POLL_MS);
}

startDashboard();
