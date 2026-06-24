const POLL_MS = 1500;
const MAX_HISTORY = 120;

const state = {
  paused: false,
  loading: false,
  timer: null,
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
};

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

function setText(node, value) {
  if (node) node.textContent = value;
}

function setGauge(node, value) {
  node?.style.setProperty("--value", String(clampPercent(value)));
}

function pushHistory(snapshot) {
  state.history.cpu.push(snapshot.cpu.usagePercent);
  state.history.ram.push(snapshot.memory.usedPercent);
  state.history.swap.push(snapshot.swap.usedPercent);
  state.history.load.push(Math.min(100, (snapshot.load.one / Math.max(1, snapshot.cpu.cores)) * 100));

  for (const values of Object.values(state.history)) {
    while (values.length > MAX_HISTORY) values.shift();
  }
}

function drawSparkline(canvas, values, color) {
  if (!canvas) return;
  const context = canvas.getContext("2d");
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth || canvas.width;
  const height = canvas.clientHeight || canvas.height;
  canvas.width = Math.floor(width * ratio);
  canvas.height = Math.floor(height * ratio);
  context.scale(ratio, ratio);
  context.clearRect(0, 0, width, height);
  context.strokeStyle = "rgba(148, 163, 184, 0.16)";
  context.lineWidth = 1;
  for (let line = 1; line < 3; line += 1) {
    const y = (height / 3) * line;
    context.beginPath();
    context.moveTo(0, y);
    context.lineTo(width, y);
    context.stroke();
  }
  if (values.length < 2) return;
  context.strokeStyle = color;
  context.lineWidth = 2;
  context.beginPath();
  values.forEach((value, index) => {
    const x = (index / (MAX_HISTORY - 1)) * width;
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
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth || canvas.width;
  const height = canvas.clientHeight || canvas.height;
  canvas.width = Math.floor(width * ratio);
  canvas.height = Math.floor(height * ratio);
  context.scale(ratio, ratio);
  context.clearRect(0, 0, width, height);

  context.strokeStyle = "rgba(148, 163, 184, 0.12)";
  context.lineWidth = 1;
  for (let line = 1; line < 5; line += 1) {
    const y = (height / 5) * line;
    context.beginPath();
    context.moveTo(0, y);
    context.lineTo(width, y);
    context.stroke();
  }

  const series = [
    ["CPU", state.history.cpu, "#22c55e"],
    ["RAM", state.history.ram, "#38bdf8"],
    ["SWAP", state.history.swap, "#a78bfa"],
    ["LOAD", state.history.load, "#f59e0b"],
  ];

  series.forEach(([label, values, color], seriesIndex) => {
    if (values.length < 2) return;
    context.strokeStyle = color;
    context.lineWidth = 2;
    context.setLineDash(label === "LOAD" ? [6, 5] : []);
    context.beginPath();
    values.forEach((value, index) => {
      const x = (index / (MAX_HISTORY - 1)) * width;
      const y = height - (clampPercent(value) / 100) * height;
      if (index === 0) context.moveTo(x, y);
      else context.lineTo(x, y);
    });
    context.stroke();
    context.setLineDash([]);
    context.fillStyle = color;
    context.fillRect(16 + seriesIndex * 92, 16, 16, 3);
    context.fillStyle = "#cbd5e1";
    context.font = "12px Inter, system-ui, sans-serif";
    context.fillText(label, 38 + seriesIndex * 92, 20);
  });
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

function renderSnapshot(snapshot) {
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

  pushHistory(snapshot);
  setText(elements.sampleCount, `${state.history.cpu.length} samples`);
  drawSparkline(elements.cpuSpark, state.history.cpu, "#22c55e");
  drawSparkline(elements.ramSpark, state.history.ram, "#38bdf8");
  drawSparkline(elements.swapSpark, state.history.swap, "#a78bfa");
  drawHistoryChart();
  renderFilesystems(snapshot.filesystems.slice(0, 8));
  renderPressure(snapshot);
  renderProcesses(snapshot.processes);
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

window.addEventListener("resize", () => {
  drawSparkline(elements.cpuSpark, state.history.cpu, "#22c55e");
  drawSparkline(elements.ramSpark, state.history.ram, "#38bdf8");
  drawSparkline(elements.swapSpark, state.history.swap, "#a78bfa");
  drawHistoryChart();
});

fetchSnapshot();
state.timer = window.setInterval(fetchSnapshot, POLL_MS);
