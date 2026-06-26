const DEFAULT_POLL_MS = 1500;
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
  visibleSeries: "tinytop.visibleSeries",
  processFilter: "tinytop.processFilter",
  processSort: "tinytop.processSort",
  processDensity: "tinytop.processDensity",
  filesystemShowSystem: "tinytop.filesystemShowSystem",
  lastSection: "tinytop.lastSection",
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
const HISTORY_SERIES_KEYS = new Set(HISTORY_METRICS.map((metric) => metric.key));
const PROCESS_SORT_KEYS = new Set(["pid", "cpu", "memory", "rss"]);
const PROCESS_DENSITIES = new Set(["comfortable", "compact"]);
const SYSTEM_FILESYSTEM_TYPES = new Set([
  "autofs",
  "binfmt_misc",
  "bpf",
  "cgroup",
  "cgroup2",
  "configfs",
  "debugfs",
  "devpts",
  "devtmpfs",
  "efivarfs",
  "fusectl",
  "mqueue",
  "proc",
  "pstore",
  "securityfs",
  "sysfs",
  "tmpfs",
  "tracefs",
]);
const DEFAULT_DAEMON_SETTINGS = {
  defaultTheme: "midnight",
  defaultGraphMode: "line",
  pollIntervalMs: DEFAULT_POLL_MS,
  defaultHistoryWindow: "live",
  retentionHours: 72,
  rollupRetentionDays: 30,
  topProcessCount: 8,
  redactionDefault: false,
  thresholds: {
    cpuWarn: 80,
    cpuCritical: 95,
    memoryWarn: 85,
    memoryCritical: 95,
    diskWarn: 85,
    diskCritical: 95,
    loadWarn: 80,
    loadCritical: 100,
    pressureWarn: 10,
    pressureCritical: 25,
  },
  enabledSections: {
    overview: true,
    history: true,
    filesystem: true,
    pressure: true,
    processes: true,
  },
};

const state = {
  paused: false,
  loading: false,
  timer: null,
  activeConfirmation: null,
  confirmationReturnFocus: null,
  settingsReturnFocus: null,
  historyChartInstance: null,
  pollMs: DEFAULT_POLL_MS,
  daemonSettings: cloneSettings(DEFAULT_DAEMON_SETTINGS),
  theme: "midnight",
  graphMode: "line",
  historyWindowKey: "live",
  visibleSeries: new Set(HISTORY_METRICS.map((metric) => metric.key)),
  processFilter: "",
  processSort: {
    key: "cpu",
    direction: "desc",
  },
  processDensity: "comfortable",
  filesystemShowSystem: false,
  timelineDragging: false,
  lastSnapshot: null,
  lastSnapshotAtMs: null,
  historyCoverage: null,
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
  daemonVersion: document.querySelector("#daemon-version"),
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
  loadGauge: document.querySelector("#load-gauge"),
  loadValue: document.querySelector("#load-value"),
  loadCapacity: document.querySelector("#load-capacity"),
  loadSpark: document.querySelector("#load-spark"),
  operatorStatus: document.querySelector("#operator-status"),
  operatorState: document.querySelector("#operator-state"),
  operatorSummary: document.querySelector("#operator-summary"),
  operatorAge: document.querySelector("#operator-age"),
  operatorOffender: document.querySelector("#operator-offender"),
  cpuPanel: document.querySelector("#cpu-panel"),
  ramPanel: document.querySelector("#ram-panel"),
  swapPanel: document.querySelector("#swap-panel"),
  loadPanel: document.querySelector("#load-panel"),
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
  filesystemShowSystem: document.querySelector("#filesystem-show-system"),
  rootFilesystemCard: document.querySelector("#root-filesystem-card"),
  rootFilesystemName: document.querySelector("#root-filesystem-name"),
  rootFilesystemUsage: document.querySelector("#root-filesystem-usage"),
  rootFilesystemBar: document.querySelector("#root-filesystem-bar"),
  pressureList: document.querySelector("#pressure-list"),
  historyChart: document.querySelector("#history-chart"),
  timelineRail: document.querySelector("#timeline-rail"),
  historyCoverage: document.querySelector("#history-coverage"),
  historyOldest: document.querySelector("#history-oldest"),
  historyNewest: document.querySelector("#history-newest"),
  historyDbSize: document.querySelector("#history-db-size"),
  historyRollups: document.querySelector("#history-rollups"),
  historySeriesInputs: Array.from(document.querySelectorAll("[data-history-series]")),
  sampleCount: document.querySelector("#sample-count"),
  processCount: document.querySelector("#process-count"),
  processRows: document.querySelector("#process-rows"),
  processPanel: document.querySelector("#processes"),
  processSearch: document.querySelector("#process-search"),
  processDensity: document.querySelector("#process-density"),
  processSortButtons: Array.from(document.querySelectorAll("[data-process-sort]")),
  processDetailDialog: document.querySelector("#process-detail-dialog"),
  processDetailTitle: document.querySelector("#process-detail-title"),
  processDetailBody: document.querySelector("#process-detail-body"),
  closeProcessDetailButton: document.querySelector("#close-process-detail-button"),
  refreshButton: document.querySelector("#refresh-button"),
  pauseButton: document.querySelector("#pause-button"),
  themeButtons: Array.from(document.querySelectorAll("[data-theme-option]")),
  graphButtons: Array.from(document.querySelectorAll("[data-graph-mode]")),
  historyWindowButtons: Array.from(document.querySelectorAll("[data-history-window]")),
  settingsOpenButton: document.querySelector("#settings-open-button"),
  settingsDialog: document.querySelector("#settings-dialog"),
  closeSettingsButton: document.querySelector("#close-settings-button"),
  cancelSettingsButton: document.querySelector("#cancel-settings-button"),
  browserThemeSetting: document.querySelector("#browser-theme-setting"),
  browserGraphSetting: document.querySelector("#browser-graph-setting"),
  browserHistoryWindowSetting: document.querySelector("#browser-history-window-setting"),
  daemonDefaultTheme: document.querySelector("#daemon-default-theme"),
  daemonDefaultGraph: document.querySelector("#daemon-default-graph"),
  daemonDefaultWindow: document.querySelector("#daemon-default-window"),
  daemonPollInterval: document.querySelector("#daemon-poll-interval"),
  daemonRetentionHours: document.querySelector("#daemon-retention-hours"),
  daemonRollupRetentionDays: document.querySelector("#daemon-rollup-retention-days"),
  daemonTopProcessCount: document.querySelector("#daemon-top-process-count"),
  daemonCpuWarn: document.querySelector("#daemon-cpu-warn"),
  daemonCpuCritical: document.querySelector("#daemon-cpu-critical"),
  daemonMemoryWarn: document.querySelector("#daemon-memory-warn"),
  daemonMemoryCritical: document.querySelector("#daemon-memory-critical"),
  daemonDiskWarn: document.querySelector("#daemon-disk-warn"),
  daemonDiskCritical: document.querySelector("#daemon-disk-critical"),
  daemonLoadWarn: document.querySelector("#daemon-load-warn"),
  daemonLoadCritical: document.querySelector("#daemon-load-critical"),
  daemonPressureWarn: document.querySelector("#daemon-pressure-warn"),
  daemonPressureCritical: document.querySelector("#daemon-pressure-critical"),
  daemonRedactionDefault: document.querySelector("#daemon-redaction-default"),
  daemonSectionOverview: document.querySelector("#daemon-section-overview"),
  daemonSectionHistory: document.querySelector("#daemon-section-history"),
  daemonSectionFilesystem: document.querySelector("#daemon-section-filesystem"),
  daemonSectionPressure: document.querySelector("#daemon-section-pressure"),
  daemonSectionProcesses: document.querySelector("#daemon-section-processes"),
  saveSettingsButton: document.querySelector("#save-settings-button"),
  settingsStatus: document.querySelector("#settings-status"),
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
  sectionNodes: {
    overview: Array.from(document.querySelectorAll('[data-section="overview"]')),
    history: Array.from(document.querySelectorAll('[data-section="history"]')),
    filesystem: Array.from(document.querySelectorAll('[data-section="filesystem"]')),
    pressure: Array.from(document.querySelectorAll('[data-section="pressure"]')),
    processes: Array.from(document.querySelectorAll('[data-section="processes"]')),
  },
  sectionLinks: Array.from(document.querySelectorAll("[data-section-link]")),
};

function readStoredValue(key, fallback, allowed) {
  try {
    const value = window.localStorage.getItem(key);
    return value && allowed.has(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

function readStoredOptionalValue(key, allowed) {
  try {
    const value = window.localStorage.getItem(key);
    return value && allowed.has(value) ? value : null;
  } catch {
    return null;
  }
}

function readStoredString(key, fallback = "") {
  try {
    return window.localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function readStoredBoolean(key, fallback = false) {
  try {
    const value = window.localStorage.getItem(key);
    if (value === "true") return true;
    if (value === "false") return false;
    return fallback;
  } catch {
    return fallback;
  }
}

function readStoredJson(key, fallback) {
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? JSON.parse(raw) : fallback;
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

function storeJson(key, value) {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // localStorage can be disabled; the controls should still work for the current session.
  }
}

function cloneSettings(settings = DEFAULT_DAEMON_SETTINGS) {
  return {
    ...settings,
    thresholds: { ...settings.thresholds },
    enabledSections: { ...settings.enabledSections },
  };
}

function normalizeThresholds(thresholds = {}) {
  return {
    ...DEFAULT_DAEMON_SETTINGS.thresholds,
    ...(thresholds ?? {}),
  };
}

function normalizeSettings(settings) {
  const fallback = cloneSettings(DEFAULT_DAEMON_SETTINGS);
  if (!settings || typeof settings !== "object") return fallback;

  return {
    ...fallback,
    ...settings,
    thresholds: normalizeThresholds(settings.thresholds),
    enabledSections: {
      ...fallback.enabledSections,
      ...(settings.enabledSections ?? {}),
    },
  };
}

function syncPressed(buttons, dataKey, activeValue) {
  for (const button of buttons) {
    button.setAttribute("aria-pressed", String(button.dataset[dataKey] === activeValue));
  }
}

function setControlValue(control, value) {
  if (control) control.value = String(value);
}

function setCheckboxValue(control, checked) {
  if (control) control.checked = Boolean(checked);
}

function applyTheme(theme, { persist = true } = {}) {
  const nextTheme = THEMES.has(theme) ? theme : "midnight";
  state.theme = nextTheme;
  document.body.dataset.theme = nextTheme;
  syncPressed(elements.themeButtons, "themeOption", nextTheme);
  setControlValue(elements.browserThemeSetting, nextTheme);
  if (persist) storeValue(STORAGE_KEYS.theme, nextTheme);
  redrawCharts();
}

function setGraphMode(mode, { persist = true } = {}) {
  const nextMode = Object.hasOwn(GRAPH_MODES, mode) ? mode : "line";
  state.graphMode = nextMode;
  syncPressed(elements.graphButtons, "graphMode", nextMode);
  setControlValue(elements.browserGraphSetting, nextMode);
  if (persist) storeValue(STORAGE_KEYS.graphMode, nextMode);
  drawHistoryChart();
}

function clampPercent(value) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function formatPercent(value) {
  return `${clampPercent(value).toFixed(value % 1 === 0 ? 0 : 1)}%`;
}

function loadPercent(snapshot) {
  return clampPercent((snapshot.load.one / Math.max(1, snapshot.cpu.cores)) * 100);
}

function metricStatus(value, warn, critical) {
  const numericValue = Number(value);
  if (!Number.isFinite(numericValue)) return "unknown";
  if (numericValue >= critical) return "critical";
  if (numericValue >= warn) return "warning";
  return "healthy";
}

function statusRank(status) {
  if (status === "critical") return 3;
  if (status === "warning") return 2;
  if (status === "stale") return 1;
  return 0;
}

function statusLabel(status) {
  if (status === "critical") return "Critical";
  if (status === "warning") return "Warning";
  if (status === "stale") return "Stale";
  if (status === "unknown") return "Unknown";
  return "Healthy";
}

function formatDurationMs(ms) {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
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

function setDatasetStatus(node, status) {
  if (node) node.setAttribute("data-status", status);
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

function closeSettingsDialog() {
  if (elements.settingsDialog?.open) {
    elements.settingsDialog.close();
  } else {
    elements.settingsDialog?.removeAttribute("open");
  }

  if (state.settingsReturnFocus instanceof HTMLElement) {
    state.settingsReturnFocus.focus();
  }
  state.settingsReturnFocus = null;
}

function openSettingsDialog() {
  if (!elements.settingsDialog) return;
  state.settingsReturnFocus = document.activeElement;
  if (typeof elements.settingsDialog.showModal === "function") {
    elements.settingsDialog.showModal();
  } else {
    elements.settingsDialog.setAttribute("open", "");
  }
  elements.closeSettingsButton?.focus();
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
    { key: "cpu", label: "CPU", values: state.history.cpu, color: palette.cpu, dashed: false },
    { key: "ram", label: "RAM", values: state.history.ram, color: palette.ram, dashed: false },
    { key: "swap", label: "SWAP", values: state.history.swap, color: palette.swap, dashed: false },
    { key: "load", label: "LOAD", values: state.history.load, color: palette.load, dashed: true },
  ];
}

function redrawCharts() {
  const palette = chartPalette();
  drawSparkline(elements.cpuSpark, state.history.cpu, palette.cpu);
  drawSparkline(elements.ramSpark, state.history.ram, palette.ram);
  drawSparkline(elements.swapSpark, state.history.swap, palette.swap);
  drawSparkline(elements.loadSpark, state.history.load, palette.load);
  drawTimelineRail();
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
    load: loadPercent(snapshot),
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

function drawTimelineRail() {
  const canvas = elements.timelineRail;
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

  context.fillStyle = cssColor("--surface-2") || "#111827";
  context.fillRect(0, 0, width, height);
  context.strokeStyle = palette.grid;
  context.lineWidth = 1;
  context.strokeRect(0.5, 0.5, width - 1, height - 1);

  const samples = state.snapshots;
  if (samples.length < 2) {
    context.fillStyle = palette.muted;
    context.font = "700 12px system-ui, sans-serif";
    context.fillText(samples.length === 1 ? "One sample loaded" : "No history loaded", 14, Math.round(height / 2));
    return;
  }

  const first = samples[0].capturedAt;
  const latest = samples[samples.length - 1].capturedAt;
  const span = Math.max(1, latest - first);
  const thresholds = normalizeThresholds(state.daemonSettings.thresholds);
  const warnY = height - (clampPercent(thresholds.cpuWarn) / 100) * height;
  const criticalY = height - (clampPercent(thresholds.cpuCritical) / 100) * height;

  context.globalAlpha = 0.32;
  context.setLineDash([5, 4]);
  context.strokeStyle = cssColor("--amber") || "#f59e0b";
  context.beginPath();
  context.moveTo(0, warnY);
  context.lineTo(width, warnY);
  context.stroke();
  context.strokeStyle = cssColor("--red") || "#ef4444";
  context.beginPath();
  context.moveTo(0, criticalY);
  context.lineTo(width, criticalY);
  context.stroke();
  context.setLineDash([]);
  context.globalAlpha = 1;

  context.strokeStyle = palette.cpu;
  context.lineWidth = 2;
  context.beginPath();
  samples.forEach((sample, index) => {
    const x = ((sample.capturedAt - first) / span) * width;
    const y = height - (clampPercent(state.history.cpu[index] ?? 0) / 100) * (height - 16) - 8;
    if (index === 0) context.moveTo(x, y);
    else context.lineTo(x, y);
  });
  context.stroke();

  const range = visibleHistoryRange();
  const visibleFirst = samples[range.start]?.capturedAt ?? first;
  const visibleLatest = samples[Math.max(range.start, range.end - 1)]?.capturedAt ?? latest;
  const selectionX = ((visibleFirst - first) / span) * width;
  const selectionWidth = Math.max(6, ((visibleLatest - visibleFirst) / span) * width);
  context.fillStyle = "rgba(56, 189, 248, 0.12)";
  context.fillRect(selectionX, 0, selectionWidth, height);

  const active = selectedSample() ?? samples[samples.length - 1];
  const markerX = ((active.capturedAt - first) / span) * width;
  context.strokeStyle = state.selectedAtMs === null ? cssColor("--green") || palette.cpu : palette.text;
  context.lineWidth = 2;
  context.beginPath();
  context.moveTo(markerX, 0);
  context.lineTo(markerX, height);
  context.stroke();
}

function timelineTimestampFromPointer(event) {
  const canvas = elements.timelineRail;
  if (!canvas || state.snapshots.length === 0) return null;
  const rect = canvas.getBoundingClientRect();
  const x = Math.max(0, Math.min(rect.width, event.clientX - rect.left));
  const first = state.snapshots[0].capturedAt;
  const latest = state.snapshots[state.snapshots.length - 1].capturedAt;
  const percent = rect.width <= 0 ? 1 : x / rect.width;
  return first + (latest - first) * percent;
}

function handleTimelinePointer(event) {
  if (event.type === "pointermove" && !state.timelineDragging) return;
  if (event.type === "pointerdown") {
    state.timelineDragging = true;
    elements.timelineRail?.setPointerCapture?.(event.pointerId);
  }
  event.preventDefault();
  const timestamp = timelineTimestampFromPointer(event);
  if (timestamp !== null) selectHistoryTimestamp(timestamp);
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
  return series
    .filter((item) => state.visibleSeries.has(item.key))
    .map((item) => ({
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

function historyChartColors(palette, series = historySeries()) {
  return series.map((item) => item.color ?? palette[item.key]).filter(Boolean);
}

function historyCategories(range) {
  return state.snapshots.slice(range.start, range.end).map((sample) => formatSampleTime(sample.capturedAt));
}

function baseCartesianOption(palette, range, series = historySeries()) {
  return {
    animation: false,
    color: historyChartColors(palette, series),
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
      data: series.map((metric) => metric.label),
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

function thresholdMarkers(item) {
  const thresholds = normalizeThresholds(state.daemonSettings.thresholds);
  const warningColor = cssColor("--amber") || "#f59e0b";
  const criticalColor = cssColor("--red") || "#ef4444";
  const values = {
    cpu: [thresholds.cpuWarn, thresholds.cpuCritical],
    ram: [thresholds.memoryWarn, thresholds.memoryCritical],
    swap: [thresholds.memoryWarn, thresholds.memoryCritical],
    load: [thresholds.loadWarn, thresholds.loadCritical],
  }[item.key];

  if (!values) return {};

  return {
    markLine: {
      silent: true,
      symbol: "none",
      data: [
        { yAxis: values[0], lineStyle: { color: warningColor, type: "dashed", opacity: 0.42 } },
        { yAxis: values[1], lineStyle: { color: criticalColor, type: "dashed", opacity: 0.5 } },
      ],
      label: { show: false },
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
    ...thresholdMarkers(item),
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
    ...thresholdMarkers(item),
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
  const base = baseCartesianOption(palette, range, series);
  return {
    ...base,
    yAxis: {
      ...base.yAxis,
      max: 100,
    },
    series: lineSeries(series),
  };
}

function buildAreaOption(palette, range, series) {
  const base = baseCartesianOption(palette, range, series);
  return {
    ...base,
    tooltip: {
      ...base.tooltip,
      order: "seriesDesc",
    },
    series: areaSeries(series),
  };
}

function buildBarOption(palette, range, series) {
  return {
    ...baseCartesianOption(palette, range, series),
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
  const colors = historyChartColors(palette, historySeries());
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
  return [
    ["CPU", snapshot.cpu.usagePercent],
    ["RAM", snapshot.memory.usedPercent],
    ["SWAP", snapshot.swap.usedPercent],
    ["LOAD", loadPercent(snapshot)],
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

  if (elements.historyChart) {
    elements.historyChart.setAttribute("aria-valuemin", String(firstSample?.capturedAt ?? 0));
    elements.historyChart.setAttribute("aria-valuemax", String(latestSample?.capturedAt ?? 0));
    elements.historyChart.setAttribute("aria-valuenow", String(activeSample?.capturedAt ?? latestSample?.capturedAt ?? 0));
    elements.historyChart.setAttribute("aria-valuetext", ariaValueText);
  }

  if (elements.timelineRail) {
    elements.timelineRail.setAttribute("aria-valuemin", String(firstSample?.capturedAt ?? 0));
    elements.timelineRail.setAttribute("aria-valuemax", String(latestSample?.capturedAt ?? 0));
    elements.timelineRail.setAttribute("aria-valuenow", String(activeSample?.capturedAt ?? latestSample?.capturedAt ?? 0));
    elements.timelineRail.setAttribute("aria-valuetext", ariaValueText);
  }

  if (elements.liveButton) {
    elements.liveButton.disabled = isLive;
  }

  if (elements.clearSessionButton) {
    elements.clearSessionButton.disabled = sampleCount === 0;
  }

  drawTimelineRail();
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

function pressureValue(snapshot, key) {
  return snapshot.pressure[key]?.some?.avg10 ?? 0;
}

function rootFilesystem(filesystems) {
  return filesystems.find((fs) => fs.mount === "/") ?? filesystems[0] ?? null;
}

function filesystemStatus(fs) {
  const thresholds = normalizeThresholds(state.daemonSettings.thresholds);
  return metricStatus(fs?.usedPercent ?? 0, thresholds.diskWarn, thresholds.diskCritical);
}

function pressureStatus(value) {
  const thresholds = normalizeThresholds(state.daemonSettings.thresholds);
  return metricStatus(value, thresholds.pressureWarn, thresholds.pressureCritical);
}

function isSystemFilesystem(fs) {
  if (!fs || fs.mount === "/") return false;
  if (SYSTEM_FILESYSTEM_TYPES.has(String(fs.type))) return true;
  const mount = String(fs.mount);
  return mount.startsWith("/proc") || mount.startsWith("/sys") || mount.startsWith("/dev");
}

function filterFilesystems(filesystems) {
  const source = Array.isArray(filesystems) ? filesystems : [];
  if (state.filesystemShowSystem) return source;
  return source.filter((fs) => !isSystemFilesystem(fs));
}

function createBar(value, status) {
  const bar = document.createElement("div");
  bar.className = "bar";
  bar.dataset.status = status;
  bar.style.setProperty("--value", String(clampPercent(value)));
  bar.append(document.createElement("span"));
  return bar;
}

function createLabelValue(labelText, valueText) {
  const row = document.createElement("div");
  row.className = "bar-label";
  const label = document.createElement("span");
  const value = document.createElement("span");
  label.textContent = labelText;
  value.textContent = valueText;
  row.append(label, value);
  return row;
}

function renderRootFilesystem(fs) {
  const status = filesystemStatus(fs);
  setDatasetStatus(elements.rootFilesystemCard, status);
  setText(elements.rootFilesystemName, fs ? `${fs.mount} on ${fs.filesystem}` : "-");
  setText(elements.rootFilesystemUsage, fs ? formatPercent(fs.usedPercent) : "-");
  if (elements.rootFilesystemBar) {
    elements.rootFilesystemBar.dataset.status = status;
    elements.rootFilesystemBar.style.setProperty("--value", String(clampPercent(fs?.usedPercent ?? 0)));
  }
}

function renderFilesystems(filesystems) {
  const visible = filterFilesystems(filesystems).slice(0, 12);
  setText(elements.filesystemCount, `${visible.length} / ${filesystems.length} mounts`);
  if (!elements.filesystemList) return;

  elements.filesystemList.replaceChildren(
    ...visible.map((fs) => {
      const item = document.createElement("div");
      const status = filesystemStatus(fs);
      item.className = "filesystem-item";
      item.dataset.status = status;

      const name = document.createElement("div");
      name.className = "fs-name";
      const mount = document.createElement("strong");
      mount.title = fs.mount;
      mount.textContent = fs.mount;
      const meta = document.createElement("span");
      meta.textContent = `${fs.filesystem} - ${fs.type} - ${formatBytes(fs.usedBytes)} / ${formatBytes(fs.sizeBytes)}`;
      name.append(mount, meta);

      const bars = document.createElement("div");
      bars.className = "bar-group";
      const capacity = document.createElement("div");
      capacity.append(createLabelValue("Capacity", formatPercent(fs.usedPercent)), createBar(fs.usedPercent, status));

      const inodeValue = fs.inodeUsedPercent ?? 0;
      const inodeStatus = fs.inodeUsedPercent === null ? "unknown" : filesystemStatus({ usedPercent: inodeValue });
      const inodes = document.createElement("div");
      inodes.append(
        createLabelValue("Inodes", fs.inodeUsedPercent === null ? "n/a" : formatPercent(inodeValue)),
        createBar(inodeValue, inodeStatus),
      );

      bars.append(capacity, inodes);
      item.append(name, bars);
      return item;
    }),
  );
}

function renderPressure(snapshot) {
  const items = [
    ["CPU", pressureValue(snapshot, "cpu"), "Scheduler contention"],
    ["Memory", pressureValue(snapshot, "memory"), "Allocation stalls"],
    ["I/O", pressureValue(snapshot, "io"), "Storage wait"],
  ];
  if (!elements.pressureList) return;
  elements.pressureList.replaceChildren(
    ...items.map(([name, value, description]) => {
      const item = document.createElement("div");
      const status = pressureStatus(value);
      item.className = "pressure-item";
      item.dataset.status = status;

      const top = document.createElement("div");
      top.className = "pressure-top";
      const strong = document.createElement("strong");
      const amount = document.createElement("span");
      strong.textContent = name;
      amount.textContent = `${Number(value).toFixed(2)} avg10`;
      top.append(strong, amount);

      const descriptionNode = document.createElement("span");
      descriptionNode.className = "label";
      descriptionNode.textContent = description;
      item.append(top, createBar(value * 5, status), descriptionNode);
      return item;
    }),
  );
}

function sortProcesses(processes) {
  const sortKey = PROCESS_SORT_KEYS.has(state.processSort.key) ? state.processSort.key : "cpu";
  const direction = state.processSort.direction === "asc" ? 1 : -1;
  return [...processes].sort((left, right) => {
    const leftValue =
      sortKey === "pid"
        ? left.pid
        : sortKey === "memory"
          ? left.memoryPercent
          : sortKey === "rss"
            ? left.rssBytes
            : left.cpuPercent;
    const rightValue =
      sortKey === "pid"
        ? right.pid
        : sortKey === "memory"
          ? right.memoryPercent
          : sortKey === "rss"
            ? right.rssBytes
            : right.cpuPercent;
    return (Number(leftValue) - Number(rightValue)) * direction;
  });
}

function filteredProcesses(processes) {
  const filter = state.processFilter.trim().toLowerCase();
  if (!filter) return processes;
  return processes.filter((process) => {
    return String(process.pid).includes(filter) || String(process.command).toLowerCase().includes(filter);
  });
}

function syncProcessSortButtons() {
  for (const button of elements.processSortButtons) {
    const active = button.dataset.processSort === state.processSort.key;
    button.setAttribute("aria-pressed", String(active));
    button.title = active ? `Sorted ${state.processSort.direction}` : "Sort";
  }
}

function renderProcessDetail(process) {
  if (!elements.processDetailDialog || !elements.processDetailBody) return;
  setText(elements.processDetailTitle, `PID ${process.pid}`);
  const rows = [
    ["Command", process.command],
    ["CPU", `${process.cpuPercent.toFixed(1)}%`],
    ["RAM", `${process.memoryPercent.toFixed(1)}%`],
    ["RSS", formatBytes(process.rssBytes)],
  ];
  elements.processDetailBody.replaceChildren(
    ...rows.map(([labelText, valueText]) => {
      const row = document.createElement("div");
      row.className = "process-detail-row";
      const label = document.createElement("span");
      const value = document.createElement("strong");
      label.textContent = labelText;
      value.textContent = valueText;
      row.append(label, value);
      return row;
    }),
  );
  if (typeof elements.processDetailDialog.showModal === "function") {
    elements.processDetailDialog.showModal();
  } else {
    elements.processDetailDialog.setAttribute("open", "");
  }
}

function renderProcesses(processes) {
  const visible = filteredProcesses(sortProcesses(processes));
  setText(elements.processCount, `${visible.length} / ${processes.length} rows`);
  syncProcessSortButtons();
  if (!elements.processRows) return;

  if (visible.length === 0) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 6;
    cell.textContent = "No matching processes";
    row.append(cell);
    elements.processRows.replaceChildren(row);
    return;
  }

  elements.processRows.replaceChildren(
    ...visible.map((process) => {
      const row = document.createElement("tr");
      const pid = document.createElement("td");
      const command = document.createElement("td");
      const cpu = document.createElement("td");
      const memory = document.createElement("td");
      const rss = document.createElement("td");
      const details = document.createElement("td");
      const detailButton = document.createElement("button");

      pid.textContent = String(process.pid);
      command.className = "command-cell";
      command.title = process.command;
      command.textContent = process.command;
      cpu.textContent = `${process.cpuPercent.toFixed(1)}%`;
      memory.textContent = `${process.memoryPercent.toFixed(1)}%`;
      rss.textContent = formatBytes(process.rssBytes);
      detailButton.className = "mini-button secondary";
      detailButton.type = "button";
      detailButton.textContent = "Open";
      detailButton.addEventListener("click", () => renderProcessDetail(process));
      details.append(detailButton);

      row.append(pid, command, cpu, memory, rss, details);
      return row;
    }),
  );
}

function applyMetricStatuses(snapshot) {
  const thresholds = normalizeThresholds(state.daemonSettings.thresholds);
  setDatasetStatus(elements.cpuPanel, metricStatus(snapshot.cpu.usagePercent, thresholds.cpuWarn, thresholds.cpuCritical));
  setDatasetStatus(elements.ramPanel, metricStatus(snapshot.memory.usedPercent, thresholds.memoryWarn, thresholds.memoryCritical));
  setDatasetStatus(elements.swapPanel, metricStatus(snapshot.swap.usedPercent, thresholds.memoryWarn, thresholds.memoryCritical));
  setDatasetStatus(elements.loadPanel, metricStatus(loadPercent(snapshot), thresholds.loadWarn, thresholds.loadCritical));
}

function computeSnapshotStatus(snapshot, nowMs = Date.now()) {
  if (!snapshot) {
    return {
      status: "stale",
      stateLabel: "Stale",
      summary: "Waiting for collector data",
      offender: "-",
      ageMs: 0,
    };
  }

  const thresholds = normalizeThresholds(state.daemonSettings.thresholds);
  const loadCritical = thresholds.loadCritical;
  const pressureCritical = thresholds.pressureCritical;
  const ageMs = Math.max(0, nowMs - snapshotCapturedAtMs(snapshot));
  const staleAfterMs = Math.max(10_000, state.pollMs * 4);
  if (ageMs > staleAfterMs) {
    return {
      status: "stale",
      stateLabel: "Stale",
      summary: "Last collector sample is old",
      offender: "collector",
      ageMs,
    };
  }

  const rootFs = rootFilesystem(snapshot.filesystems);
  const pressureMax = Math.max(pressureValue(snapshot, "cpu"), pressureValue(snapshot, "memory"), pressureValue(snapshot, "io"));
  const candidates = [
    {
      name: "CPU",
      value: snapshot.cpu.usagePercent,
      formatted: formatPercent(snapshot.cpu.usagePercent),
      status: metricStatus(snapshot.cpu.usagePercent, thresholds.cpuWarn, thresholds.cpuCritical),
    },
    {
      name: "RAM",
      value: snapshot.memory.usedPercent,
      formatted: formatPercent(snapshot.memory.usedPercent),
      status: metricStatus(snapshot.memory.usedPercent, thresholds.memoryWarn, thresholds.memoryCritical),
    },
    {
      name: "Load",
      value: loadPercent(snapshot),
      formatted: formatPercent(loadPercent(snapshot)),
      status: metricStatus(loadPercent(snapshot), thresholds.loadWarn, loadCritical),
    },
    {
      name: "Disk",
      value: rootFs?.usedPercent ?? 0,
      formatted: rootFs ? formatPercent(rootFs.usedPercent) : "-",
      status: rootFs ? metricStatus(rootFs.usedPercent, thresholds.diskWarn, thresholds.diskCritical) : "unknown",
    },
    {
      name: "PSI",
      value: pressureMax,
      formatted: `${pressureMax.toFixed(2)} avg10`,
      status: metricStatus(pressureMax, thresholds.pressureWarn, pressureCritical),
    },
  ];
  const worst = candidates.sort((left, right) => statusRank(right.status) - statusRank(left.status) || right.value - left.value)[0];
  const status = worst?.status === "unknown" ? "healthy" : (worst?.status ?? "healthy");
  return {
    status,
    stateLabel: statusLabel(status),
    summary: status === "healthy" ? "All tracked thresholds are below warning" : `${worst.name} at ${worst.formatted}`,
    offender: status === "healthy" ? "-" : worst.name,
    ageMs,
  };
}

function renderOperatorStatus(snapshot, override) {
  const result = override ?? computeSnapshotStatus(snapshot);
  setDatasetStatus(elements.operatorStatus, result.status);
  setText(elements.operatorState, result.stateLabel ?? statusLabel(result.status));
  setText(elements.operatorSummary, result.summary ?? "-");
  setText(elements.operatorOffender, result.offender ?? "-");
  setText(elements.operatorAge, formatDurationMs(result.ageMs ?? 0));
}

function renderSnapshotDetails(snapshot) {
  const rootFs = rootFilesystem(snapshot.filesystems);
  const loadPressure = loadPercent(snapshot);

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

  setText(elements.loadValue, formatPercent(loadPressure));
  setText(elements.loadCapacity, `${snapshot.load.one.toFixed(2)} / ${snapshot.cpu.cores} cores`);
  setGauge(elements.loadGauge, loadPressure);

  setText(elements.loadOne, snapshot.load.one.toFixed(2));
  setText(elements.loadContext, `${snapshot.load.five.toFixed(2)} / ${snapshot.load.fifteen.toFixed(2)} 5m/15m`);
  setText(elements.threadCount, String(snapshot.load.totalThreads));
  setText(elements.runnableCount, `${snapshot.load.runnable} runnable`);
  setText(elements.rootUsed, rootFs ? formatPercent(rootFs.usedPercent) : "-");
  setText(elements.rootMount, rootFs ? `${rootFs.mount} on ${rootFs.filesystem}` : "-");

  applyMetricStatuses(snapshot);
  renderRootFilesystem(rootFs);
  renderFilesystems(snapshot.filesystems);
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

function versionComponentLabel(metadata) {
  if (metadata.runtime === "rust") return "Rust collector/dashboard";
  if (metadata.component === "collector") return "Legacy Bun collector";
  return "Legacy Bun dashboard";
}

function renderVersion(metadata) {
  const label = versionComponentLabel(metadata);
  const version = metadata.version ? `v${metadata.version}` : "version unknown";
  setText(elements.daemonVersion, `${label} ${version}`);
  if (elements.daemonVersion) {
    elements.daemonVersion.title = metadata.dashboard ? `Dashboard assets: ${metadata.dashboard}` : label;
  }
}

async function fetchVersion() {
  try {
    const response = await fetch("/api/version", { cache: "no-store" });
    if (!response.ok) throw new Error(`Version failed with HTTP ${response.status}`);
    renderVersion(await response.json());
  } catch {
    setText(elements.daemonVersion, "Version unavailable");
  }
}

function renderSettingsStatus(message) {
  setText(elements.settingsStatus, message);
}

function applyEnabledSections(settings) {
  const enabledSections = {
    ...DEFAULT_DAEMON_SETTINGS.enabledSections,
    ...(settings.enabledSections ?? {}),
  };

  for (const [section, nodes] of Object.entries(elements.sectionNodes)) {
    const enabled = Boolean(enabledSections[section]);
    for (const node of nodes) setHidden(node, !enabled);
  }

  for (const link of elements.sectionLinks) {
    const enabled = Boolean(enabledSections[link.dataset.sectionLink]);
    setHidden(link, !enabled);
  }
}

function populateDaemonSettings(settings) {
  const nextSettings = normalizeSettings(settings);
  state.daemonSettings = cloneSettings(nextSettings);
  state.pollMs = Math.max(250, Number(nextSettings.pollIntervalMs) || DEFAULT_POLL_MS);

  setControlValue(elements.daemonDefaultTheme, nextSettings.defaultTheme);
  setControlValue(elements.daemonDefaultGraph, nextSettings.defaultGraphMode);
  setControlValue(elements.daemonDefaultWindow, nextSettings.defaultHistoryWindow);
  setControlValue(elements.daemonPollInterval, nextSettings.pollIntervalMs);
  setControlValue(elements.daemonRetentionHours, nextSettings.retentionHours);
  setControlValue(elements.daemonRollupRetentionDays, nextSettings.rollupRetentionDays);
  setControlValue(elements.daemonTopProcessCount, nextSettings.topProcessCount);
  setControlValue(elements.daemonCpuWarn, nextSettings.thresholds.cpuWarn);
  setControlValue(elements.daemonCpuCritical, nextSettings.thresholds.cpuCritical);
  setControlValue(elements.daemonMemoryWarn, nextSettings.thresholds.memoryWarn);
  setControlValue(elements.daemonMemoryCritical, nextSettings.thresholds.memoryCritical);
  setControlValue(elements.daemonDiskWarn, nextSettings.thresholds.diskWarn);
  setControlValue(elements.daemonDiskCritical, nextSettings.thresholds.diskCritical);
  setControlValue(elements.daemonLoadWarn, nextSettings.thresholds.loadWarn);
  setControlValue(elements.daemonLoadCritical, nextSettings.thresholds.loadCritical);
  setControlValue(elements.daemonPressureWarn, nextSettings.thresholds.pressureWarn);
  setControlValue(elements.daemonPressureCritical, nextSettings.thresholds.pressureCritical);
  setCheckboxValue(elements.daemonRedactionDefault, nextSettings.redactionDefault);
  setCheckboxValue(elements.daemonSectionOverview, nextSettings.enabledSections.overview);
  setCheckboxValue(elements.daemonSectionHistory, nextSettings.enabledSections.history);
  setCheckboxValue(elements.daemonSectionFilesystem, nextSettings.enabledSections.filesystem);
  setCheckboxValue(elements.daemonSectionPressure, nextSettings.enabledSections.pressure);
  setCheckboxValue(elements.daemonSectionProcesses, nextSettings.enabledSections.processes);
  applyEnabledSections(nextSettings);
  if (state.lastSnapshot) renderOperatorStatus(state.lastSnapshot);
}

function numberControlValue(control, fallback) {
  const value = Number(control?.value);
  return Number.isFinite(value) ? Math.round(value) : fallback;
}

function collectDaemonSettingsFromForm() {
  return {
    ...cloneSettings(state.daemonSettings),
    defaultTheme: elements.daemonDefaultTheme?.value ?? "midnight",
    defaultGraphMode: elements.daemonDefaultGraph?.value ?? "line",
    defaultHistoryWindow: elements.daemonDefaultWindow?.value ?? "live",
    pollIntervalMs: numberControlValue(elements.daemonPollInterval, DEFAULT_POLL_MS),
    retentionHours: numberControlValue(elements.daemonRetentionHours, 72),
    rollupRetentionDays: numberControlValue(elements.daemonRollupRetentionDays, 30),
    topProcessCount: numberControlValue(elements.daemonTopProcessCount, 8),
    redactionDefault: Boolean(elements.daemonRedactionDefault?.checked),
    thresholds: {
      cpuWarn: numberControlValue(elements.daemonCpuWarn, 80),
      cpuCritical: numberControlValue(elements.daemonCpuCritical, 95),
      memoryWarn: numberControlValue(elements.daemonMemoryWarn, 85),
      memoryCritical: numberControlValue(elements.daemonMemoryCritical, 95),
      diskWarn: numberControlValue(elements.daemonDiskWarn, 85),
      diskCritical: numberControlValue(elements.daemonDiskCritical, 95),
      loadWarn: numberControlValue(elements.daemonLoadWarn, 80),
      loadCritical: numberControlValue(elements.daemonLoadCritical, 100),
      pressureWarn: numberControlValue(elements.daemonPressureWarn, 10),
      pressureCritical: numberControlValue(elements.daemonPressureCritical, 25),
    },
    enabledSections: {
      overview: Boolean(elements.daemonSectionOverview?.checked),
      history: Boolean(elements.daemonSectionHistory?.checked),
      filesystem: Boolean(elements.daemonSectionFilesystem?.checked),
      pressure: Boolean(elements.daemonSectionPressure?.checked),
      processes: Boolean(elements.daemonSectionProcesses?.checked),
    },
  };
}

function readStoredVisibleSeries() {
  const stored = readStoredJson(STORAGE_KEYS.visibleSeries, null);
  if (!Array.isArray(stored)) return new Set(HISTORY_METRICS.map((metric) => metric.key));
  const values = stored.filter((key) => HISTORY_SERIES_KEYS.has(key));
  return new Set(values.length === 0 ? HISTORY_METRICS.map((metric) => metric.key) : values);
}

function syncVisibleSeriesControls() {
  for (const input of elements.historySeriesInputs) {
    input.checked = state.visibleSeries.has(input.dataset.historySeries);
  }
}

function persistVisibleSeries() {
  storeJson(STORAGE_KEYS.visibleSeries, Array.from(state.visibleSeries));
}

function syncProcessControls() {
  setControlValue(elements.processSearch, state.processFilter);
  setControlValue(elements.processDensity, state.processDensity);
  if (elements.processPanel) elements.processPanel.dataset.density = state.processDensity;
  syncProcessSortButtons();
}

function applyInitialBrowserSettings(settings) {
  const graphModes = new Set(Object.keys(GRAPH_MODES));
  applyTheme(readStoredValue(STORAGE_KEYS.theme, settings.defaultTheme, THEMES), { persist: false });
  setGraphMode(readStoredValue(STORAGE_KEYS.graphMode, settings.defaultGraphMode, graphModes), { persist: false });
  setHistoryWindow(readStoredValue(STORAGE_KEYS.historyWindow, settings.defaultHistoryWindow, HISTORY_WINDOW_KEYS), {
    fetch: false,
    persist: false,
  });
  state.visibleSeries = readStoredVisibleSeries();
  syncVisibleSeriesControls();
  state.processFilter = readStoredString(STORAGE_KEYS.processFilter, "");
  const storedSort = readStoredJson(STORAGE_KEYS.processSort, state.processSort);
  if (storedSort && PROCESS_SORT_KEYS.has(storedSort.key)) {
    state.processSort = {
      key: storedSort.key,
      direction: storedSort.direction === "asc" ? "asc" : "desc",
    };
  }
  state.processDensity = readStoredValue(STORAGE_KEYS.processDensity, "comfortable", PROCESS_DENSITIES);
  state.filesystemShowSystem = readStoredBoolean(STORAGE_KEYS.filesystemShowSystem, false);
  setCheckboxValue(elements.filesystemShowSystem, state.filesystemShowSystem);
  syncProcessControls();
}

async function fetchSettings() {
  try {
    const response = await fetch("/api/settings", { cache: "no-store" });
    if (!response.ok) throw new Error(`Settings failed with HTTP ${response.status}`);
    const settings = normalizeSettings(await response.json());
    populateDaemonSettings(settings);
    renderSettingsStatus("Daemon defaults loaded.");
    return settings;
  } catch (error) {
    const settings = cloneSettings(DEFAULT_DAEMON_SETTINGS);
    populateDaemonSettings(settings);
    renderSettingsStatus(error instanceof Error ? error.message : "Settings unavailable.");
    return settings;
  }
}

function restartPollingTimer() {
  if (!state.timer) return;
  window.clearInterval(state.timer);
  state.timer = window.setInterval(fetchSnapshot, state.pollMs);
}

async function saveDaemonSettings() {
  const settings = collectDaemonSettingsFromForm();
  if (elements.saveSettingsButton) elements.saveSettingsButton.disabled = true;
  renderSettingsStatus("Saving daemon defaults.");
  try {
    const response = await fetch("/api/settings", {
      method: "PUT",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify(settings),
    });
    if (!response.ok) throw new Error(`Settings save failed with HTTP ${response.status}`);
    const saved = normalizeSettings(await response.json());
    populateDaemonSettings(saved);
    restartPollingTimer();
    fetchHistoryCoverage();
    renderSettingsStatus("Daemon defaults saved.");
  } catch (error) {
    renderSettingsStatus(error instanceof Error ? error.message : "Settings save failed.");
  } finally {
    if (elements.saveSettingsButton) elements.saveSettingsButton.disabled = false;
  }
}

async function fetchSnapshot() {
  if (state.loading || state.paused) return;
  state.loading = true;
  try {
    const response = await fetch("/api/snapshot", { cache: "no-store" });
    if (!response.ok) throw new Error(`Snapshot failed with HTTP ${response.status}`);
    const snapshot = await response.json();
    state.lastSnapshot = snapshot;
    state.lastSnapshotAtMs = Date.now();
    setHidden(elements.statusMessage, true);
    renderSnapshot(snapshot);
    renderOperatorStatus(snapshot);
    fetchHistoryCoverage();
    setLiveStatus("live", "Live");
  } catch (error) {
    setHidden(elements.statusMessage, false);
    setText(elements.statusMessage, error instanceof Error ? error.message : "Snapshot failed");
    renderOperatorStatus(null, {
      status: "critical",
      stateLabel: "Critical",
      summary: error instanceof Error ? error.message : "Snapshot failed",
      offender: "collector",
      ageMs: state.lastSnapshotAtMs ? Date.now() - state.lastSnapshotAtMs : 0,
    });
    setLiveStatus("error", "Error");
  } finally {
    state.loading = false;
  }
}

async function fetchHistory() {
  return fetchHistoryWindow();
}

function formatCoverageTime(timestampMs) {
  const numeric = Number(timestampMs);
  return Number.isFinite(numeric) && numeric > 0 ? formatSampleDateTime(numeric) : "-";
}

function renderHistoryCoverage(coverage) {
  state.historyCoverage = coverage;
  setText(elements.historyOldest, formatCoverageTime(coverage?.oldestCapturedAtMs));
  setText(elements.historyNewest, formatCoverageTime(coverage?.newestCapturedAtMs));
  setText(elements.historyDbSize, formatBytes(Number(coverage?.databaseBytes ?? 0)));
  setText(elements.historyRollups, `${Number(coverage?.rollupBucketCount ?? 0)} buckets`);
}

async function fetchHistoryCoverage() {
  try {
    const response = await fetch("/api/history/coverage", { cache: "no-store" });
    if (!response.ok) throw new Error(`History coverage failed with HTTP ${response.status}`);
    const coverage = await response.json();
    renderHistoryCoverage(coverage);
    return coverage;
  } catch {
    renderHistoryCoverage(null);
    return null;
  }
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
  setControlValue(elements.browserHistoryWindowSetting, nextWindow);
  if (persist) storeValue(STORAGE_KEYS.historyWindow, nextWindow);
  updateHistoryControls();
  if (fetch) {
    fetchHistoryWindow();
    fetchHistoryCoverage();
  }
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

elements.settingsOpenButton?.addEventListener("click", () => {
  openSettingsDialog();
});

elements.closeSettingsButton?.addEventListener("click", () => {
  closeSettingsDialog();
});

elements.cancelSettingsButton?.addEventListener("click", () => {
  closeSettingsDialog();
});

elements.settingsDialog?.addEventListener("cancel", (event) => {
  event.preventDefault();
  closeSettingsDialog();
});

elements.settingsDialog?.addEventListener("click", (event) => {
  if (event.target === elements.settingsDialog) closeSettingsDialog();
});

elements.browserThemeSetting?.addEventListener("change", () => {
  applyTheme(elements.browserThemeSetting.value);
});

elements.browserGraphSetting?.addEventListener("change", () => {
  setGraphMode(elements.browserGraphSetting.value);
});

elements.browserHistoryWindowSetting?.addEventListener("change", () => {
  setHistoryWindow(elements.browserHistoryWindowSetting.value);
});

elements.saveSettingsButton?.addEventListener("click", () => {
  saveDaemonSettings();
});

for (const input of elements.historySeriesInputs) {
  input.addEventListener("change", () => {
    const key = input.dataset.historySeries;
    if (!HISTORY_SERIES_KEYS.has(key)) return;
    if (input.checked) state.visibleSeries.add(key);
    else state.visibleSeries.delete(key);
    if (state.visibleSeries.size === 0) {
      state.visibleSeries.add(key);
      input.checked = true;
    }
    persistVisibleSeries();
    redrawCharts();
  });
}

elements.filesystemShowSystem?.addEventListener("change", () => {
  state.filesystemShowSystem = Boolean(elements.filesystemShowSystem.checked);
  storeValue(STORAGE_KEYS.filesystemShowSystem, String(state.filesystemShowSystem));
  renderSelectedSample();
});

elements.processSearch?.addEventListener("input", () => {
  state.processFilter = elements.processSearch.value;
  storeValue(STORAGE_KEYS.processFilter, state.processFilter);
  renderSelectedSample();
});

elements.processDensity?.addEventListener("change", () => {
  state.processDensity = PROCESS_DENSITIES.has(elements.processDensity.value) ? elements.processDensity.value : "comfortable";
  storeValue(STORAGE_KEYS.processDensity, state.processDensity);
  syncProcessControls();
});

for (const button of elements.processSortButtons) {
  button.addEventListener("click", () => {
    const key = button.dataset.processSort;
    if (!PROCESS_SORT_KEYS.has(key)) return;
    state.processSort = {
      key,
      direction: state.processSort.key === key && state.processSort.direction === "desc" ? "asc" : "desc",
    };
    storeJson(STORAGE_KEYS.processSort, state.processSort);
    renderSelectedSample();
  });
}

elements.closeProcessDetailButton?.addEventListener("click", () => {
  if (elements.processDetailDialog?.open) elements.processDetailDialog.close();
  else elements.processDetailDialog?.removeAttribute("open");
});

elements.processDetailDialog?.addEventListener("cancel", (event) => {
  event.preventDefault();
  if (elements.processDetailDialog.open) elements.processDetailDialog.close();
});

elements.processDetailDialog?.addEventListener("click", (event) => {
  if (event.target === elements.processDetailDialog) elements.processDetailDialog.close();
});

elements.timelineRail?.addEventListener("pointerdown", handleTimelinePointer);
elements.timelineRail?.addEventListener("pointermove", handleTimelinePointer);
elements.timelineRail?.addEventListener("pointerup", (event) => {
  state.timelineDragging = false;
  elements.timelineRail?.releasePointerCapture?.(event.pointerId);
});
elements.timelineRail?.addEventListener("pointercancel", () => {
  state.timelineDragging = false;
});

for (const link of elements.sectionLinks) {
  link.addEventListener("click", () => {
    storeValue(STORAGE_KEYS.lastSection, link.dataset.sectionLink ?? "");
  });
}

elements.timelineRail?.addEventListener("keydown", (event) => {
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

async function startDashboard() {
  const settings = await fetchSettings();
  applyInitialBrowserSettings(settings);
  await fetchVersion();
  await fetchHistory();
  await fetchHistoryCoverage();
  await fetchSnapshot();
  state.timer = window.setInterval(fetchSnapshot, state.pollMs);
}

startDashboard();
