const POLL_MS = 1500;
const MAX_HISTORY = 120;
const STORAGE_KEYS = {
  theme: "wsl-status-dashboard.theme",
  graphMode: "wsl-status-dashboard.graphMode",
};
const THEMES = new Set(["midnight", "matrix", "aurora", "solar", "ember"]);
const GRAPH_MODES = {
  line: "Line graph",
  area: "Area graph",
  bar: "Bar graph",
  heatmap: "Heatmap",
};

const state = {
  paused: false,
  loading: false,
  timer: null,
  theme: "midnight",
  graphMode: "line",
  snapshots: [],
  selectedSampleIndex: null,
  history: {
    cpu: [],
    ram: [],
    swap: [],
    load: [],
  },
};

const elements = {
  alert: document.querySelector("#alert"),
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
  historyScrubber: document.querySelector("#history-scrubber"),
  historyPositionLabel: document.querySelector("#history-position-label"),
  historyStartLabel: document.querySelector("#history-start-label"),
  historyEndLabel: document.querySelector("#history-end-label"),
  historySampleValues: document.querySelector("#history-sample-values"),
  liveButton: document.querySelector("#live-button"),
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

function setGauge(node, value) {
  node?.style.setProperty("--value", String(clampPercent(value)));
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

function pushHistory(snapshot) {
  state.history.cpu.push(snapshot.cpu.usagePercent);
  state.history.ram.push(snapshot.memory.usedPercent);
  state.history.swap.push(snapshot.swap.usedPercent);
  state.history.load.push(Math.min(100, (snapshot.load.one / Math.max(1, snapshot.cpu.cores)) * 100));
  state.snapshots.push({
    capturedAt: Date.now(),
    snapshot,
  });

  for (const values of Object.values(state.history)) {
    while (values.length > MAX_HISTORY) values.shift();
  }

  while (state.snapshots.length > MAX_HISTORY) {
    state.snapshots.shift();
    if (state.selectedSampleIndex !== null) {
      state.selectedSampleIndex = Math.max(0, state.selectedSampleIndex - 1);
    }
  }
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

function drawHistoryChart() {
  const canvas = elements.historyChart;
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
  context.globalAlpha = 0.4;
  context.lineWidth = 1;
  for (let line = 1; line < 5; line += 1) {
    const y = (height / 5) * line;
    context.beginPath();
    context.moveTo(0, y);
    context.lineTo(width, y);
    context.stroke();
  }
  context.globalAlpha = 1;

  const series = historySeries();
  if (state.graphMode === "area") drawAreaHistory(context, width, height, series);
  else if (state.graphMode === "bar") drawBarHistory(context, width, height, series);
  else if (state.graphMode === "heatmap") drawHeatmapHistory(context, width, height, series, palette);
  else drawLineHistory(context, width, height, series);

  if (state.graphMode !== "heatmap") drawHistoryAxisLabels(context, width, height, palette);
  drawHistoryLegend(context, series, palette);
  drawHistoryMarker(context, width, height, palette);
}

function drawLineHistory(context, width, height, series) {
  for (const item of series) {
    if (item.values.length < 2) continue;
    context.strokeStyle = item.color;
    context.lineWidth = 2;
    context.setLineDash(item.dashed ? [6, 5] : []);
    context.beginPath();
    const denominator = Math.max(1, item.values.length - 1);
    item.values.forEach((value, index) => {
      const x = (index / denominator) * width;
      const y = height - (clampPercent(value) / 100) * height;
      if (index === 0) context.moveTo(x, y);
      else context.lineTo(x, y);
    });
    context.stroke();
    context.setLineDash([]);
  }
}

function drawAreaHistory(context, width, height, series) {
  for (const item of series) {
    if (item.values.length < 2) continue;
    const gradient = context.createLinearGradient(0, 0, 0, height);
    gradient.addColorStop(0, item.color);
    gradient.addColorStop(1, "transparent");
    context.globalAlpha = item.label === "LOAD" ? 0.16 : 0.22;
    context.fillStyle = gradient;
    context.beginPath();
    const denominator = Math.max(1, item.values.length - 1);
    item.values.forEach((value, index) => {
      const x = (index / denominator) * width;
      const y = height - (clampPercent(value) / 100) * height;
      if (index === 0) context.moveTo(x, y);
      else context.lineTo(x, y);
    });
    context.lineTo(width, height);
    context.lineTo(0, height);
    context.closePath();
    context.fill();
    context.globalAlpha = 1;
  }
  drawLineHistory(context, width, height, series);
}

function drawBarHistory(context, width, height, series) {
  const topOffset = 36;
  const bottomOffset = 8;
  const plotHeight = Math.max(1, height - topOffset - bottomOffset);
  const visibleCount = Math.min(Math.max(...series.map((item) => item.values.length), 1), 80);
  const groupWidth = width / visibleCount;
  const groupGap = Math.min(5, Math.max(1, groupWidth * 0.16));
  const availableWidth = Math.max(1, groupWidth - groupGap * 2);
  const barWidth = Math.max(1, availableWidth / series.length);

  series.forEach((item, seriesIndex) => {
    const values = item.values.slice(-visibleCount);
    values.forEach((value, index) => {
      const normalized = clampPercent(value) / 100;
      const barHeight = normalized * plotHeight;
      const x = index * groupWidth + groupGap + seriesIndex * barWidth;
      const y = topOffset + plotHeight - barHeight;

      context.fillStyle = item.color;
      context.globalAlpha = item.label === "LOAD" ? 0.72 : 0.9;
      context.fillRect(x, y, Math.max(1, barWidth - 1), Math.max(1, barHeight));
    });
  });
  context.globalAlpha = 1;
}

function drawHeatmapHistory(context, width, height, series, palette) {
  const topOffset = 36;
  const rowGap = 8;
  const labelWidth = 54;
  const valueWidth = 56;
  const rowHeight = Math.max(20, (height - topOffset - rowGap * (series.length - 1) - 12) / series.length);
  const visibleCount = Math.min(Math.max(...series.map((item) => item.values.length), 1), 80);
  const cellAreaWidth = Math.max(1, width - labelWidth - valueWidth - 20);
  const cellWidth = cellAreaWidth / visibleCount;

  context.font = "12px Inter, system-ui, sans-serif";
  series.forEach((item, rowIndex) => {
    const y = topOffset + rowIndex * (rowHeight + rowGap);
    const values = item.values.slice(-visibleCount);
    const latestValue = values.at(-1);

    context.fillStyle = item.color;
    context.globalAlpha = 0.08;
    context.fillRect(labelWidth, y, cellAreaWidth, rowHeight);
    context.globalAlpha = 0.95;
    context.fillText(item.label, 12, y + rowHeight * 0.62);

    values.forEach((value, index) => {
      const intensity = 0.08 + (clampPercent(value) / 100) * 0.82;
      context.globalAlpha = intensity;
      context.fillStyle = item.color;
      context.fillRect(labelWidth + index * cellWidth, y, Math.max(1, cellWidth - 1), rowHeight);
    });

    context.globalAlpha = 0.86;
    context.fillStyle = palette.text;
    context.fillText(Number.isFinite(latestValue) ? formatPercent(latestValue) : "-", width - valueWidth + 8, y + rowHeight * 0.62);
  });
  context.globalAlpha = 1;
}

function drawHistoryLegend(context, series, palette) {
  context.font = "12px Inter, system-ui, sans-serif";
  series.forEach((item, seriesIndex) => {
    const x = 16 + seriesIndex * 92;
    context.fillStyle = item.color;
    context.fillRect(x, 16, 16, 3);
    context.fillStyle = palette.text;
    context.globalAlpha = 0.82;
    context.fillText(item.label, x + 22, 20);
  });
  context.globalAlpha = 1;
}

function drawHistoryAxisLabels(context, width, height, palette) {
  context.save();
  context.font = "11px Inter, system-ui, sans-serif";
  context.textAlign = "right";
  context.fillStyle = palette.muted;
  context.globalAlpha = 0.76;
  for (let line = 1; line < 5; line += 1) {
    const y = (height / 5) * line;
    context.fillText(`${100 - line * 20}%`, width - 8, y - 4);
  }
  context.fillText("0%", width - 8, height - 7);
  context.restore();
}

function drawHistoryMarker(context, width, height, palette) {
  if (state.selectedSampleIndex === null || state.snapshots.length < 2) return;
  const denominator = Math.max(1, state.snapshots.length - 1);
  const x = (state.selectedSampleIndex / denominator) * width;
  context.save();
  context.strokeStyle = palette.text;
  context.globalAlpha = 0.68;
  context.lineWidth = 1.5;
  context.setLineDash([4, 5]);
  context.beginPath();
  context.moveTo(x, 0);
  context.lineTo(x, height);
  context.stroke();
  context.restore();
}

function selectedSample() {
  if (state.snapshots.length === 0) return null;
  const fallbackIndex = state.snapshots.length - 1;
  const index = state.selectedSampleIndex ?? fallbackIndex;
  return state.snapshots[Math.max(0, Math.min(index, fallbackIndex))];
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

function updateHistoryControls() {
  const sampleCount = state.snapshots.length;
  const lastIndex = Math.max(0, sampleCount - 1);
  const activeIndex = state.selectedSampleIndex ?? lastIndex;
  const activeSample = selectedSample();
  const firstSample = state.snapshots[0];
  const latestSample = state.snapshots[lastIndex];
  const isLive = state.selectedSampleIndex === null;
  const positionText =
    activeSample && isLive
      ? `Live - ${formatSampleDateTime(activeSample.capturedAt)}`
      : activeSample
        ? `Viewing ${formatSampleDateTime(activeSample.capturedAt)}`
        : "Live";
  const ariaValueText =
    activeSample && isLive
      ? `Live, latest sample ${formatSampleDateTime(activeSample.capturedAt)}`
      : activeSample
        ? `Sample ${activeIndex + 1} of ${sampleCount}, ${formatSampleDateTime(activeSample.capturedAt)}`
        : "Live";

  setText(elements.sampleCount, `${sampleCount} ${sampleCount === 1 ? "sample" : "samples"}`);
  setText(elements.historyPositionLabel, positionText);
  setText(elements.historyStartLabel, firstSample ? formatSampleDateTime(firstSample.capturedAt) : "-");
  setText(elements.historyEndLabel, latestSample ? `Live - ${formatSampleTime(latestSample.capturedAt)}` : "Live");
  renderHistorySampleValues(activeSample);

  if (elements.historyScrubber) {
    elements.historyScrubber.disabled = sampleCount < 2;
    elements.historyScrubber.min = "0";
    elements.historyScrubber.max = String(lastIndex);
    elements.historyScrubber.value = String(activeIndex);
    elements.historyScrubber.setAttribute("aria-valuetext", ariaValueText);
  }

  if (elements.liveButton) {
    elements.liveButton.disabled = isLive;
  }
}

function renderSelectedSample() {
  const sample = selectedSample();
  if (!sample) return;
  renderSnapshotDetails(sample.snapshot);
  updateHistoryControls();
}

function setHistorySelection(index) {
  if (state.snapshots.length === 0) return;
  const lastIndex = state.snapshots.length - 1;
  state.selectedSampleIndex = Math.max(0, Math.min(index, lastIndex));
  renderSelectedSample();
  redrawCharts();
}

function returnToLiveHistory() {
  state.selectedSampleIndex = null;
  renderSelectedSample();
  redrawCharts();
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
    elements.alert.hidden = true;
    renderSnapshot(snapshot);
    setLiveStatus("live", "Live");
  } catch (error) {
    elements.alert.hidden = false;
    setText(elements.alert, error instanceof Error ? error.message : "Snapshot failed");
    setLiveStatus("error", "Error");
  } finally {
    state.loading = false;
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

elements.historyScrubber?.addEventListener("input", () => {
  setHistorySelection(Number(elements.historyScrubber.value));
});

elements.liveButton?.addEventListener("click", () => {
  returnToLiveHistory();
});

window.addEventListener("resize", () => {
  redrawCharts();
});

applyTheme(readStoredValue(STORAGE_KEYS.theme, "midnight", THEMES));
setGraphMode(readStoredValue(STORAGE_KEYS.graphMode, "line", new Set(Object.keys(GRAPH_MODES))));
fetchSnapshot();
state.timer = window.setInterval(fetchSnapshot, POLL_MS);
