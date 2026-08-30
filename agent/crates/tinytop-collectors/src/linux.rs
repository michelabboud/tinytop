use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use procfs::{
    CpuPressure, IoPressure, KernelStats, LoadAverage, Meminfo, MemoryPressure, Uptime, prelude::*,
};
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, ProcessRefreshKind, ProcessesToUpdate, System,
    UpdateKind,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureLine, PressureSnapshot, ProcessSnapshot, RuntimeConfidence,
    RuntimeDetection, RuntimeKind, SwapSnapshot, SystemSnapshot,
};

use crate::{
    Collector, CollectorConfig, CollectorError, CollectorResult,
    gpu::{GpuAdapter, GpuBackend, GpuScanStats, attach_gpu, detect_backend},
    thermal::{self, ThermalSensor},
};

#[derive(Debug, Clone)]
pub struct LinuxSnapshotSources {
    pub timestamp: String,
    pub filesystems_captured_at_ms: i64,
    pub hostname: String,
    pub platform: String,
    pub arch: String,
    pub cpu_count: usize,
    pub os_release_text: String,
    pub proc_version: String,
    pub kernel_release: String,
    pub wsl_distro_name: Option<String>,
    pub wsl_interop: Option<String>,
    pub uptime_text: String,
    pub meminfo_text: String,
    pub loadavg_text: String,
    pub previous_proc_stat_text: String,
    pub current_proc_stat_text: String,
    pub cpu_pressure_text: String,
    pub memory_pressure_text: String,
    pub io_pressure_text: String,
    pub df_blocks_text: String,
    pub df_inodes_text: String,
    pub ps_text: String,
}

#[derive(Debug, Clone)]
pub struct LinuxFastSources {
    pub timestamp: String,
    pub cpu_count: usize,
    pub uptime_text: String,
    pub meminfo_text: String,
    pub loadavg_text: String,
    pub previous_proc_stat_text: String,
    pub current_proc_stat_text: String,
    pub cpu_pressure_text: String,
    pub memory_pressure_text: String,
    pub io_pressure_text: String,
    pub ps_text: String,
}

#[derive(Debug, Clone)]
pub struct LinuxSlowSources {
    pub captured_at_ms: i64,
    pub hostname: String,
    pub platform: String,
    pub arch: String,
    pub os_release_text: String,
    pub proc_version: String,
    pub kernel_release: String,
    pub wsl_distro_name: Option<String>,
    pub wsl_interop: Option<String>,
    pub df_blocks_text: String,
    pub df_inodes_text: String,
}

struct LinuxSlowCache {
    taken_at: Instant,
    captured_at_ms: i64,
    sources: LinuxSlowSources,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMeminfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub buffers_bytes: u64,
    pub cached_bytes: u64,
    pub used_bytes: u64,
    pub used_percent: f64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_used_percent: f64,
}

type GpuDetector = Box<dyn FnMut() -> Option<Box<dyn GpuBackend>> + Send>;
const DEFAULT_THERMAL_ROOT: &str = "/sys/class/hwmon";

pub struct LinuxCollector {
    previous_proc_stat_text: Option<String>,
    system: System,
    config: CollectorConfig,
    slow_cache: Option<LinuxSlowCache>,
    clock: Box<dyn FnMut() -> Instant + Send>,
    slow_enumerations: u64,
    configure_calls: u64,
    gpu: Option<Box<dyn GpuBackend>>,
    gpu_adapters: Vec<GpuAdapter>,
    gpu_detector: Option<GpuDetector>,
    thermal_sensors: Vec<ThermalSensor>,
    thermal_root: PathBuf,
    thermal_enabled: bool,
    thermal_extra_chips: Vec<String>,
    #[cfg(test)]
    thermal_scan_calls: u64,
}

impl Default for LinuxCollector {
    fn default() -> Self {
        Self {
            previous_proc_stat_text: None,
            system: System::new(),
            config: CollectorConfig::default(),
            slow_cache: None,
            clock: Box::new(Instant::now),
            slow_enumerations: 0,
            configure_calls: 0,
            gpu: detect_backend(),
            gpu_adapters: Vec::new(),
            gpu_detector: Some(Box::new(detect_backend)),
            thermal_sensors: Vec::new(),
            thermal_root: PathBuf::from(DEFAULT_THERMAL_ROOT),
            thermal_enabled: false,
            thermal_extra_chips: Vec::new(),
            #[cfg(test)]
            thermal_scan_calls: 0,
        }
    }
}

impl LinuxCollector {
    fn without_gpu(clock: Box<dyn FnMut() -> Instant + Send>) -> Self {
        Self {
            previous_proc_stat_text: None,
            system: System::new(),
            config: CollectorConfig::default(),
            slow_cache: None,
            clock,
            slow_enumerations: 0,
            configure_calls: 0,
            gpu: None,
            gpu_adapters: Vec::new(),
            gpu_detector: None,
            thermal_sensors: Vec::new(),
            thermal_root: PathBuf::from(DEFAULT_THERMAL_ROOT),
            thermal_enabled: false,
            thermal_extra_chips: Vec::new(),
            #[cfg(test)]
            thermal_scan_calls: 0,
        }
    }

    #[doc(hidden)]
    pub fn with_clock(clock: impl FnMut() -> Instant + Send + 'static) -> Self {
        Self::without_gpu(Box::new(clock))
    }

    #[doc(hidden)]
    pub fn with_gpu_backend(backend: Box<dyn GpuBackend>) -> Self {
        let mut collector = Self::without_gpu(Box::new(Instant::now));
        collector.gpu = Some(backend);
        collector
    }

    #[doc(hidden)]
    pub fn last_gpu_scan(&self) -> Option<GpuScanStats> {
        self.gpu.as_ref().and_then(|backend| backend.last_scan())
    }

    #[doc(hidden)]
    pub fn slow_enumerations(&self) -> u64 {
        self.slow_enumerations
    }

    #[doc(hidden)]
    pub fn configure_calls(&self) -> u64 {
        self.configure_calls
    }

    pub fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        if env::consts::OS != "linux" {
            return Err(CollectorError::UnsupportedPlatform {
                platform: env::consts::OS,
            });
        }

        let now = (self.clock)();
        let first_slow_tick = self.slow_cache.is_none();
        let slow_due = self.slow_cache.as_ref().is_none_or(|cache| {
            now.duration_since(cache.taken_at) >= self.config.filesystems_interval
        });
        if slow_due {
            let sources = collect_slow_sources()?;
            self.slow_enumerations = self.slow_enumerations.saturating_add(1);
            self.slow_cache = Some(LinuxSlowCache {
                taken_at: now,
                captured_at_ms: sources.captured_at_ms,
                sources,
            });
            if let Some(gpu) = &mut self.gpu {
                self.gpu_adapters = gpu.detect();
            } else if !first_slow_tick && let Some(detector) = &mut self.gpu_detector {
                self.gpu = detector();
                if let Some(gpu) = &mut self.gpu {
                    self.gpu_adapters = gpu.detect();
                }
            }
            if self.thermal_enabled {
                #[cfg(test)]
                {
                    self.thermal_scan_calls = self.thermal_scan_calls.saturating_add(1);
                }
                let scan = thermal::scan(&self.thermal_root, &self.thermal_extra_chips);
                self.thermal_sensors = scan.sensors;
            } else {
                self.thermal_sensors.clear();
            }
        }

        let fast = collect_fast_sources(
            &mut self.system,
            self.previous_proc_stat_text.as_deref(),
            self.config.top_process_count,
        )?;
        let gpu_tick = self.gpu.as_mut().map(|gpu| {
            let samples = gpu.sample();
            let busy = gpu.process_busy();
            (samples, busy)
        });
        let sensors = if self.thermal_enabled {
            thermal::read_values(&self.thermal_root, &self.thermal_sensors)
        } else {
            Vec::new()
        };
        self.previous_proc_stat_text = Some(fast.current_proc_stat_text.clone());
        let cache = self
            .slow_cache
            .as_ref()
            .expect("the first collection always populates the slow cache");
        let sources = merge_sources(fast, cache.sources.clone(), cache.captured_at_ms);
        let mut snapshot = build_linux_snapshot_from_sources(sources)?;
        if let Some((samples, busy)) = gpu_tick {
            attach_gpu(&mut snapshot, &self.gpu_adapters, &samples, &busy);
        }
        snapshot.sensors = sensors;
        Ok(snapshot)
    }
}

impl Collector for LinuxCollector {
    fn configure(&mut self, config: CollectorConfig) {
        self.configure_calls = self.configure_calls.saturating_add(1);
        self.thermal_enabled = config.thermal_enabled;
        self.thermal_extra_chips
            .clone_from(&config.thermal_extra_chips);
        if self.config != config {
            self.config = config;
        }
    }

    fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        Self::collect(self)
    }
}

pub fn build_linux_snapshot_from_sources(
    sources: LinuxSnapshotSources,
) -> CollectorResult<SystemSnapshot> {
    let memory = parse_meminfo(&sources.meminfo_text)?;
    let previous_cpu = parse_proc_stat(&sources.previous_proc_stat_text)?;
    let current_cpu = parse_proc_stat(&sources.current_proc_stat_text)?;
    let filesystems = merge_filesystems(
        parse_df_blocks(&sources.df_blocks_text)?,
        &sources.df_inodes_text,
    )?;
    let runtime = detect_linux_runtime(
        &sources.kernel_release,
        &sources.proc_version,
        sources.wsl_distro_name.as_deref(),
        sources.wsl_interop.as_deref(),
    );

    Ok(SystemSnapshot {
        timestamp: sources.timestamp,
        filesystems_captured_at_ms: Some(sources.filesystems_captured_at_ms),
        identity: IdentitySnapshot {
            hostname: sources.hostname,
            platform: sources.platform,
            arch: sources.arch,
            distro: parse_pretty_name(&sources.os_release_text),
            kernel: sources.kernel_release,
            runtime,
            uptime_seconds: parse_uptime(&sources.uptime_text),
        },
        cpu: CpuSnapshot {
            usage_percent: calculate_cpu_usage(&previous_cpu, &current_cpu),
            cores: sources.cpu_count,
            times: Some(current_cpu),
        },
        memory: MemorySnapshot {
            total_bytes: memory.total_bytes,
            available_bytes: memory.available_bytes,
            used_bytes: memory.used_bytes,
            used_percent: memory.used_percent,
        },
        swap: SwapSnapshot {
            total_bytes: memory.swap_total_bytes,
            free_bytes: memory.swap_free_bytes,
            used_bytes: memory.swap_used_bytes,
            used_percent: memory.swap_used_percent,
        },
        load: parse_loadavg(&sources.loadavg_text)?,
        pressure: PressureGroup {
            cpu: parse_pressure(&sources.cpu_pressure_text)?,
            memory: parse_pressure(&sources.memory_pressure_text)?,
            io: parse_pressure(&sources.io_pressure_text)?,
        },
        filesystems,
        processes: parse_processes(&sources.ps_text),
        gpus: Vec::new(),
        sensors: Vec::new(),
    })
}

pub fn collect_fast_sources(
    system: &mut System,
    previous_proc_stat_text: Option<&str>,
    top_process_count: usize,
) -> CollectorResult<LinuxFastSources> {
    let first_proc_stat = match previous_proc_stat_text {
        Some(text) => text.to_string(),
        None => proc_stat_text()?,
    };

    let meminfo = Meminfo::current()?;
    refresh_system(system);

    if previous_proc_stat_text.is_none() {
        thread::sleep(Duration::from_millis(120));
        refresh_system(system);
    }
    let current_proc_stat = proc_stat_text()?;

    // Store validation guarantees at least one process; retain the invariant at
    // the collector boundary if a future caller bypasses validated settings.
    let top_process_count = top_process_count.max(1);
    Ok(LinuxFastSources {
        timestamp: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| CollectorError::parse("format timestamp", error.to_string()))?,
        cpu_count: system.cpus().len().max(1),
        uptime_text: procfs_uptime_text()?,
        meminfo_text: procfs_meminfo_text(&meminfo),
        loadavg_text: procfs_loadavg_text()?,
        previous_proc_stat_text: first_proc_stat,
        current_proc_stat_text: current_proc_stat,
        cpu_pressure_text: cpu_pressure_text(),
        memory_pressure_text: memory_pressure_text(),
        io_pressure_text: io_pressure_text(),
        ps_text: sysinfo_process_text(system, meminfo.mem_total, top_process_count),
    })
}

pub fn collect_slow_sources() -> CollectorResult<LinuxSlowSources> {
    let disks = Disks::new_with_refreshed_list_specifics(
        DiskRefreshKind::nothing().with_kind().with_storage(),
    );
    Ok(LinuxSlowSources {
        captured_at_ms: now_unix_ms()?,
        hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
        platform: "linux".to_string(),
        arch: env::consts::ARCH.to_string(),
        os_release_text: sysinfo_os_release_text(),
        proc_version: System::kernel_version().unwrap_or_default(),
        kernel_release: System::kernel_version().unwrap_or_else(|| env::consts::OS.to_string()),
        wsl_distro_name: env::var("WSL_DISTRO_NAME").ok(),
        wsl_interop: env::var("WSL_INTEROP").ok(),
        df_blocks_text: sysinfo_df_blocks_text(&disks),
        df_inodes_text: statvfs_inodes_text(&disks),
    })
}

/// Collect every source in one shot, including a fresh filesystem enumeration.
/// This convenience is intentionally uncached; `LinuxCollector` owns cadence.
pub fn collect_sources(
    system: &mut System,
    previous_proc_stat_text: Option<&str>,
) -> CollectorResult<LinuxSnapshotSources> {
    let fast = collect_fast_sources(
        system,
        previous_proc_stat_text,
        CollectorConfig::default().top_process_count,
    )?;
    let slow = collect_slow_sources()?;
    let captured_at_ms = slow.captured_at_ms;
    Ok(merge_sources(fast, slow, captured_at_ms))
}

fn merge_sources(
    fast: LinuxFastSources,
    slow: LinuxSlowSources,
    filesystems_captured_at_ms: i64,
) -> LinuxSnapshotSources {
    LinuxSnapshotSources {
        timestamp: fast.timestamp,
        filesystems_captured_at_ms,
        hostname: slow.hostname,
        platform: slow.platform,
        arch: slow.arch,
        cpu_count: fast.cpu_count,
        os_release_text: slow.os_release_text,
        proc_version: slow.proc_version,
        kernel_release: slow.kernel_release,
        wsl_distro_name: slow.wsl_distro_name,
        wsl_interop: slow.wsl_interop,
        uptime_text: fast.uptime_text,
        meminfo_text: fast.meminfo_text,
        loadavg_text: fast.loadavg_text,
        previous_proc_stat_text: fast.previous_proc_stat_text,
        current_proc_stat_text: fast.current_proc_stat_text,
        cpu_pressure_text: fast.cpu_pressure_text,
        memory_pressure_text: fast.memory_pressure_text,
        io_pressure_text: fast.io_pressure_text,
        df_blocks_text: slow.df_blocks_text,
        df_inodes_text: slow.df_inodes_text,
        ps_text: fast.ps_text,
    }
}

fn now_unix_ms() -> CollectorResult<i64> {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| CollectorError::parse("read timestamp", "milliseconds exceed i64"))
}

pub fn parse_meminfo(text: &str) -> CollectorResult<ParsedMeminfo> {
    let mut values = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        let key = key.trim_end_matches(':');
        if let Ok(kib) = value.parse::<u64>() {
            values.insert(key.to_string(), kib.saturating_mul(1024));
        }
    }

    let total_bytes = *values.get("MemTotal").unwrap_or(&0);
    if total_bytes == 0 {
        return Err(CollectorError::parse(
            "parse /proc/meminfo",
            "MemTotal missing or zero",
        ));
    }

    let free_bytes = *values.get("MemFree").unwrap_or(&0);
    let available_bytes = *values.get("MemAvailable").unwrap_or(&free_bytes);
    let buffers_bytes = *values.get("Buffers").unwrap_or(&0);
    let cached_bytes = *values.get("Cached").unwrap_or(&0);
    let used_bytes = total_bytes.saturating_sub(available_bytes);
    let swap_total_bytes = *values.get("SwapTotal").unwrap_or(&0);
    let swap_free_bytes = *values.get("SwapFree").unwrap_or(&0);
    let swap_used_bytes = swap_total_bytes.saturating_sub(swap_free_bytes);

    Ok(ParsedMeminfo {
        total_bytes,
        free_bytes,
        available_bytes,
        buffers_bytes,
        cached_bytes,
        used_bytes,
        used_percent: percent(used_bytes, total_bytes),
        swap_total_bytes,
        swap_free_bytes,
        swap_used_bytes,
        swap_used_percent: percent(swap_used_bytes, swap_total_bytes),
    })
}

pub fn parse_loadavg(text: &str) -> CollectorResult<LoadSnapshot> {
    let parts = text.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 5 {
        return Err(CollectorError::parse(
            "parse /proc/loadavg",
            "expected five fields",
        ));
    }
    let thread_counts = parts[3].split('/').collect::<Vec<_>>();

    Ok(LoadSnapshot {
        one: parse_f64(parts[0], "load one")?,
        five: parse_f64(parts[1], "load five")?,
        fifteen: parse_f64(parts[2], "load fifteen")?,
        runnable: Some(parse_u64(
            thread_counts.first().copied().unwrap_or("0"),
            "runnable",
        )?),
        total_threads: Some(parse_u64(
            thread_counts.get(1).copied().unwrap_or("0"),
            "total threads",
        )?),
        last_pid: Some(parse_u64(parts[4], "last pid")?),
    })
}

pub fn parse_proc_stat(text: &str) -> CollectorResult<CpuTimes> {
    let line = text
        .lines()
        .find(|candidate| candidate.starts_with("cpu "))
        .ok_or_else(|| CollectorError::parse("parse /proc/stat", "aggregate cpu line missing"))?;
    let mut values = line
        .split_whitespace()
        .skip(1)
        .map(|value| parse_u64(value, "cpu time"));

    let user = values.next().transpose()?.unwrap_or(0);
    let nice = values.next().transpose()?.unwrap_or(0);
    let system = values.next().transpose()?.unwrap_or(0);
    let idle = values.next().transpose()?.unwrap_or(0);
    let iowait = values.next().transpose()?.unwrap_or(0);
    let irq = values.next().transpose()?.unwrap_or(0);
    let softirq = values.next().transpose()?.unwrap_or(0);
    let steal = values.next().transpose()?.unwrap_or(0);
    let guest = values.next().transpose()?.unwrap_or(0);
    let guest_nice = values.next().transpose()?.unwrap_or(0);
    let idle_total = idle.saturating_add(iowait);
    let total = user
        .saturating_add(nice)
        .saturating_add(system)
        .saturating_add(idle)
        .saturating_add(iowait)
        .saturating_add(irq)
        .saturating_add(softirq)
        .saturating_add(steal)
        .saturating_add(guest)
        .saturating_add(guest_nice);

    Ok(CpuTimes {
        user,
        nice,
        system,
        idle,
        iowait,
        irq,
        softirq,
        steal,
        guest,
        guest_nice,
        total,
        idle_total,
    })
}

pub fn calculate_cpu_usage(previous: &CpuTimes, current: &CpuTimes) -> f64 {
    let total_delta = current.total.saturating_sub(previous.total);
    let idle_delta = current.idle_total.saturating_sub(previous.idle_total);
    if total_delta == 0 {
        return 0.0;
    }
    round_percent(((total_delta.saturating_sub(idle_delta)) as f64 / total_delta as f64) * 100.0)
}

pub fn parse_pressure(text: &str) -> CollectorResult<PressureSnapshot> {
    let mut snapshot = PressureSnapshot::default();

    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let Some(label) = parts.next() else {
            continue;
        };
        if label != "some" && label != "full" {
            continue;
        }

        let mut values = HashMap::new();
        for pair in parts {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            values.insert(key, value);
        }

        let pressure_line = PressureLine {
            avg10: parse_f64(
                values.get("avg10").copied().unwrap_or("0"),
                "pressure avg10",
            )?,
            avg60: parse_f64(
                values.get("avg60").copied().unwrap_or("0"),
                "pressure avg60",
            )?,
            avg300: parse_f64(
                values.get("avg300").copied().unwrap_or("0"),
                "pressure avg300",
            )?,
            total: parse_u64(
                values.get("total").copied().unwrap_or("0"),
                "pressure total",
            )?,
        };

        match label {
            "some" => snapshot.some = Some(pressure_line),
            "full" => snapshot.full = Some(pressure_line),
            _ => {}
        }
    }

    Ok(snapshot)
}

pub fn parse_df_blocks(text: &str) -> CollectorResult<Vec<FilesystemSnapshot>> {
    let mut filesystems = Vec::new();

    for line in text.lines().skip(1).filter(|line| !line.trim().is_empty()) {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 7 {
            continue;
        }
        let columns = DfColumns::from_right(&parts)?;

        filesystems.push(FilesystemSnapshot {
            filesystem: columns.filesystem,
            fs_type: columns.fs_type,
            size_bytes: parse_u64(columns.total, "filesystem size")?,
            used_bytes: parse_u64(columns.used, "filesystem used")?,
            available_bytes: parse_u64(columns.available, "filesystem available")?,
            used_percent: parse_percent_text(columns.percent, "filesystem percent")?,
            mount: columns.mount,
            inode_used_percent: None,
            inode_used: None,
            inode_total: None,
        });
    }

    Ok(filesystems)
}

pub fn detect_linux_runtime(
    os_release: &str,
    proc_version: &str,
    wsl_distro_name: Option<&str>,
    wsl_interop: Option<&str>,
) -> RuntimeDetection {
    let release = os_release.to_lowercase();
    let version = proc_version.to_lowercase();

    if release.contains("microsoft")
        || release.contains("wsl")
        || version.contains("microsoft")
        || version.contains("wsl")
    {
        return RuntimeDetection {
            kind: RuntimeKind::Wsl,
            confidence: RuntimeConfidence::High,
            reason: "kernel release/version contains Microsoft WSL markers".to_string(),
        };
    }

    if wsl_distro_name.is_some() || wsl_interop.is_some() {
        return RuntimeDetection {
            kind: RuntimeKind::Wsl,
            confidence: RuntimeConfidence::Medium,
            reason: "WSL environment variables are present".to_string(),
        };
    }

    if !release.is_empty() || !version.is_empty() {
        return RuntimeDetection {
            kind: RuntimeKind::Linux,
            confidence: RuntimeConfidence::High,
            reason: "no WSL kernel or environment markers detected".to_string(),
        };
    }

    RuntimeDetection {
        kind: RuntimeKind::Unknown,
        confidence: RuntimeConfidence::Low,
        reason: "kernel release and version were unavailable".to_string(),
    }
}

fn merge_filesystems(
    mut filesystems: Vec<FilesystemSnapshot>,
    inode_text: &str,
) -> CollectorResult<Vec<FilesystemSnapshot>> {
    let inodes = parse_inodes(inode_text)?;
    for filesystem in &mut filesystems {
        if let Some(inode) = inodes.get(&filesystem.mount) {
            filesystem.inode_total = Some(inode.inode_total);
            filesystem.inode_used = Some(inode.inode_used);
            filesystem.inode_used_percent = Some(inode.inode_used_percent);
        }
    }
    Ok(filesystems)
}

#[derive(Debug, Clone)]
struct InodeSnapshot {
    inode_total: u64,
    inode_used: u64,
    inode_used_percent: f64,
}

fn parse_inodes(text: &str) -> CollectorResult<HashMap<String, InodeSnapshot>> {
    let mut inodes = HashMap::new();

    for line in text.lines().skip(1).filter(|line| !line.trim().is_empty()) {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 7 {
            continue;
        }
        let columns = DfColumns::from_right(&parts)?;
        let Ok(inode_total) = parse_u64(columns.total, "inode total") else {
            continue;
        };
        let Ok(inode_used) = parse_u64(columns.used, "inode used") else {
            continue;
        };
        let Ok(inode_used_percent) = parse_percent_text(columns.percent, "inode percent") else {
            continue;
        };

        inodes.insert(
            columns.mount,
            InodeSnapshot {
                inode_total,
                inode_used,
                inode_used_percent,
            },
        );
    }

    Ok(inodes)
}

struct DfColumns<'a> {
    filesystem: String,
    fs_type: String,
    total: &'a str,
    used: &'a str,
    available: &'a str,
    percent: &'a str,
    mount: String,
}

impl<'a> DfColumns<'a> {
    fn from_right(parts: &[&'a str]) -> CollectorResult<Self> {
        if parts.len() < 7 {
            return Err(CollectorError::parse(
                "parse df",
                "expected at least seven columns",
            ));
        }

        let split = parts.len() - 6;
        Ok(Self {
            filesystem: parts[..split].join(" "),
            fs_type: parts[split].to_string(),
            total: parts[split + 1],
            used: parts[split + 2],
            available: parts[split + 3],
            percent: parts[split + 4],
            mount: parts[split + 5].to_string(),
        })
    }
}

fn parse_processes(text: &str) -> Vec<ProcessSnapshot> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            if line.contains('\t') {
                let parts = line.splitn(7, '\t').collect::<Vec<_>>();
                if parts.len() < 7 {
                    return None;
                }

                return Some(ProcessSnapshot {
                    pid: parts[0].parse().ok()?,
                    cpu_percent: parts[1].parse().ok()?,
                    memory_percent: parts[2].parse().ok()?,
                    rss_bytes: parts[3].parse::<u64>().ok()?.saturating_mul(1024),
                    parent_pid: parse_optional_u32(parts[4]),
                    started_at: parse_optional_string(parts[5]),
                    gpu_percent: None,
                    command: parts[6].to_string(),
                });
            }

            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 5 {
                return None;
            }

            Some(ProcessSnapshot {
                pid: parts[0].parse().ok()?,
                cpu_percent: parts[1].parse().ok()?,
                memory_percent: parts[2].parse().ok()?,
                rss_bytes: parts[3].parse::<u64>().ok()?.saturating_mul(1024),
                parent_pid: None,
                started_at: None,
                gpu_percent: None,
                command: parts[4..].join(" "),
            })
        })
        .collect()
}

fn parse_pretty_name(os_release_text: &str) -> String {
    os_release_text
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key == "PRETTY_NAME").then(|| value.trim_matches('"').to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Linux".to_string())
}

fn parse_uptime(text: &str) -> u64 {
    text.split_whitespace()
        .next()
        .and_then(|seconds| seconds.parse::<f64>().ok())
        .map(|seconds| seconds.floor().max(0.0) as u64)
        .unwrap_or(0)
}

fn refresh_system(system: &mut System) {
    system.refresh_cpu_list(CpuRefreshKind::nothing().with_cpu_usage());
    system.refresh_cpu_usage();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .without_tasks(),
    );
}

fn proc_stat_text() -> CollectorResult<String> {
    let stats = KernelStats::current()?;
    Ok(format!(
        "cpu  {} {} {} {} {} {} {} {} {} {}\n",
        stats.total.user,
        stats.total.nice,
        stats.total.system,
        stats.total.idle,
        stats.total.iowait.unwrap_or(0),
        stats.total.irq.unwrap_or(0),
        stats.total.softirq.unwrap_or(0),
        stats.total.steal.unwrap_or(0),
        stats.total.guest.unwrap_or(0),
        stats.total.guest_nice.unwrap_or(0),
    ))
}

fn procfs_uptime_text() -> CollectorResult<String> {
    let uptime = Uptime::current()?;
    Ok(format!("{} {}", uptime.uptime, uptime.idle))
}

fn sysinfo_os_release_text() -> String {
    let pretty = System::long_os_version()
        .or_else(System::name)
        .unwrap_or_else(|| "Linux".to_string());
    format!("PRETTY_NAME=\"{pretty}\"")
}

fn procfs_meminfo_text(meminfo: &Meminfo) -> String {
    format!(
        "MemTotal:       {} kB\nMemFree:        {} kB\nMemAvailable:   {} kB\nBuffers:        {} kB\nCached:         {} kB\nSwapTotal:      {} kB\nSwapFree:       {} kB\n",
        meminfo.mem_total / 1024,
        meminfo.mem_free / 1024,
        meminfo.mem_available.unwrap_or(meminfo.mem_free) / 1024,
        meminfo.buffers / 1024,
        meminfo.cached / 1024,
        meminfo.swap_total / 1024,
        meminfo.swap_free / 1024,
    )
}

fn procfs_loadavg_text() -> CollectorResult<String> {
    let load = LoadAverage::current()?;
    Ok(format!(
        "{} {} {} {}/{} {}",
        load.one, load.five, load.fifteen, load.cur, load.max, load.latest_pid
    ))
}

fn sysinfo_df_blocks_text(disks: &Disks) -> String {
    let mut lines = vec!["Filesystem Type 1-blocks Used Available Use% Mounted on".to_string()];
    for disk in disks.list() {
        let size = disk.total_space();
        let available = disk.available_space();
        let used = size.saturating_sub(available);
        let used_percent = percent(used, size);
        lines.push(format!(
            "{} {} {} {} {} {}% {}",
            os_str_to_string(disk.name()),
            os_str_to_string(disk.file_system()),
            size,
            used,
            available,
            used_percent,
            path_to_string(disk.mount_point()),
        ));
    }
    lines.join("\n")
}

/// Build `df -Pi -T`-compatible inode text from a `statvfs(2)` call per mount.
///
/// The Bun collector shells out to `df -Pi -T` for inode usage; the Rust
/// collector deliberately avoids subprocesses (ADR 0005/0012 and the
/// `linux_collector_does_not_shell_out_for_host_metrics` guard test), so it reads
/// the same numbers directly via `statvfs`: `f_files` is the total inode count and
/// `f_ffree` the free count. We iterate the *same* `Disks` list already used for
/// the block-usage text so the mount strings match exactly and
/// [`merge_filesystems`] can join blocks to inodes by mount. The output is fed
/// through the existing [`parse_inodes`] path unchanged.
///
/// A mount we cannot stat (permission denied, disappeared, or an unresponsive
/// network filesystem) is simply omitted, leaving its inode fields null — the
/// same result the previous `String::new()` placeholder produced.
fn statvfs_inodes_text(disks: &Disks) -> String {
    let mut lines = vec!["Filesystem Type Inodes IUsed IFree IUse% Mounted on".to_string()];
    for disk in disks.list() {
        let mount = disk.mount_point();
        let Ok(stat) = rustix::fs::statvfs(mount) else {
            continue;
        };
        let total = stat.f_files;
        let free = stat.f_ffree;
        let used = total.saturating_sub(free);
        lines.push(format!(
            "{} {} {} {} {} {}% {}",
            os_str_to_string(disk.name()),
            os_str_to_string(disk.file_system()),
            total,
            used,
            free,
            percent(used, total),
            path_to_string(mount),
        ));
    }
    lines.join("\n")
}

fn sysinfo_process_text(system: &System, total_memory: u64, top_process_count: usize) -> String {
    let mut processes = system
        .processes()
        .values()
        .map(|process| {
            let memory_percent = if total_memory == 0 {
                0.0
            } else {
                round_percent((process.memory() as f64 / total_memory as f64) * 100.0)
            };
            (
                process.pid().as_u32(),
                process.cpu_usage() as f64,
                memory_percent,
                process.memory() / 1024,
                process.parent().map(|pid| pid.as_u32()),
                process_started_at(process),
                process_command(process),
            )
        })
        .collect::<Vec<_>>();
    processes.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    processes
        .into_iter()
        .take(top_process_count)
        .map(
            |(pid, cpu, memory, rss_kib, parent_pid, started_at, command)| {
                format!(
                    "{pid}\t{cpu:.1}\t{memory:.1}\t{rss_kib}\t{}\t{}\t{command}",
                    parent_pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    started_at.unwrap_or_else(|| "-".to_string()),
                )
            },
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn process_command(process: &sysinfo::Process) -> String {
    if !process.cmd().is_empty() {
        return process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
    }

    os_str_to_string(process.name())
}

fn process_started_at(process: &sysinfo::Process) -> Option<String> {
    let start_time = process.start_time();
    if start_time == 0 {
        return None;
    }
    OffsetDateTime::from_unix_timestamp(i64::try_from(start_time).ok()?)
        .ok()
        .and_then(|time| time.format(&Rfc3339).ok())
}

fn parse_optional_u32(value: &str) -> Option<u32> {
    (value != "-").then(|| value.parse().ok()).flatten()
}

fn parse_optional_string(value: &str) -> Option<String> {
    (value != "-" && !value.is_empty()).then(|| value.to_string())
}

fn cpu_pressure_text() -> String {
    CpuPressure::current()
        .map(|pressure| pressure_snapshot_text(Some(pressure.some), None))
        .unwrap_or_default()
}

fn memory_pressure_text() -> String {
    MemoryPressure::current()
        .map(|pressure| pressure_snapshot_text(Some(pressure.some), Some(pressure.full)))
        .unwrap_or_default()
}

fn io_pressure_text() -> String {
    IoPressure::current()
        .map(|pressure| pressure_snapshot_text(Some(pressure.some), Some(pressure.full)))
        .unwrap_or_default()
}

fn pressure_snapshot_text(
    some: Option<procfs::PressureRecord>,
    full: Option<procfs::PressureRecord>,
) -> String {
    let mut lines = Vec::new();
    if let Some(record) = some {
        lines.push(pressure_record_text("some", record));
    }
    if let Some(record) = full {
        lines.push(pressure_record_text("full", record));
    }
    lines.join("\n")
}

fn pressure_record_text(label: &str, record: procfs::PressureRecord) -> String {
    format!(
        "{label} avg10={} avg60={} avg300={} total={}",
        record.avg10, record.avg60, record.avg300, record.total,
    )
}

fn os_str_to_string(value: &OsStr) -> String {
    decode_proc_mount_escape(&value.to_string_lossy())
}

fn path_to_string(value: &Path) -> String {
    decode_proc_mount_escape(&value.to_string_lossy())
}

pub fn decode_proc_mount_escape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &bytes[index + 1..index + 4];
            if octal.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
                let value = ((octal[0] - b'0') << 6) | ((octal[1] - b'0') << 3) | (octal[2] - b'0');
                decoded.push(value as char);
                index += 4;
                continue;
            }
        }

        let Some(character) = value[index..].chars().next() else {
            break;
        };
        decoded.push(character);
        index += character.len_utf8();
    }

    decoded
}

fn parse_u64(value: &str, context: &'static str) -> CollectorResult<u64> {
    value
        .parse::<u64>()
        .map_err(|error| CollectorError::parse(context, error.to_string()))
}

fn parse_f64(value: &str, context: &'static str) -> CollectorResult<f64> {
    value
        .parse::<f64>()
        .map_err(|error| CollectorError::parse(context, error.to_string()))
}

fn parse_percent_text(value: &str, context: &'static str) -> CollectorResult<f64> {
    parse_f64(value.trim_end_matches('%'), context)
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        round_percent((used as f64 / total as f64) * 100.0)
    }
}

fn round_percent(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

#[cfg(test)]
mod gpu_tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use serde_json::json;

    use super::LinuxCollector;
    use crate::{
        Collector, CollectorConfig,
        gpu::{GpuAdapter, GpuBackend, GpuSample, GpuScanStats, GpuVendor, attach_gpu},
    };

    #[derive(Default)]
    struct Counts {
        detect: usize,
        sample: usize,
    }

    struct FakeBackend {
        counts: Arc<Mutex<Counts>>,
        busy: HashMap<u32, f64>,
    }

    impl GpuBackend for FakeBackend {
        fn detect(&mut self) -> Vec<GpuAdapter> {
            self.counts.lock().expect("counts mutex").detect += 1;
            vec![GpuAdapter {
                id: "pci-0000:02:00.0".to_string(),
                vendor: GpuVendor::Amd,
                name: "fixture GPU".to_string(),
                driver: "amdgpu".to_string(),
            }]
        }

        fn sample(&mut self) -> Vec<GpuSample> {
            self.counts.lock().expect("counts mutex").sample += 1;
            vec![GpuSample {
                adapter_id: "pci-0000:02:00.0".to_string(),
                busy_percent: Some(25.0),
                memory_used_bytes: None,
                memory_total_bytes: None,
                temperature_c: None,
            }]
        }

        fn process_busy(&mut self) -> HashMap<u32, f64> {
            self.busy.clone()
        }

        fn last_scan(&self) -> Option<GpuScanStats> {
            None
        }
    }

    #[test]
    fn gpu_is_redetected_on_the_slow_tick_and_sampled_every_tick() {
        let counts = Arc::new(Mutex::new(Counts::default()));
        let backend = FakeBackend {
            counts: Arc::clone(&counts),
            busy: HashMap::new(),
        };
        let base = Instant::now();
        let offset = Arc::new(Mutex::new(Duration::ZERO));
        let clock_offset = Arc::clone(&offset);
        let clocked =
            LinuxCollector::with_clock(move || base + *clock_offset.lock().expect("clock mutex"));
        let mut collector = LinuxCollector::with_gpu_backend(Box::new(backend));
        collector.clock = clocked.clock;
        collector.configure(CollectorConfig {
            filesystems_interval: Duration::from_secs(60),
            ..CollectorConfig::default()
        });

        collector.collect().expect("first GPU collection");
        assert_eq!(counts.lock().expect("counts mutex").detect, 1);
        assert_eq!(counts.lock().expect("counts mutex").sample, 1);

        *offset.lock().expect("clock mutex") = Duration::from_secs(30);
        collector.collect().expect("fast GPU collection");
        assert_eq!(counts.lock().expect("counts mutex").detect, 1);
        assert_eq!(counts.lock().expect("counts mutex").sample, 2);

        *offset.lock().expect("clock mutex") = Duration::from_secs(61);
        collector.collect().expect("slow GPU collection");
        assert_eq!(counts.lock().expect("counts mutex").detect, 2);
        assert_eq!(counts.lock().expect("counts mutex").sample, 3);
    }

    #[test]
    fn gpu_is_discovered_after_a_cold_empty_start_on_a_later_slow_tick() {
        // Break caught: storing `None` after startup made later hotplug invisible forever.
        let counts = Arc::new(Mutex::new(Counts::default()));
        let detector_calls = Arc::new(Mutex::new(0_usize));
        let detector_count = Arc::clone(&detector_calls);
        let backend_counts = Arc::clone(&counts);
        let base = Instant::now();
        let offset = Arc::new(Mutex::new(Duration::ZERO));
        let clock_offset = Arc::clone(&offset);
        let mut collector =
            LinuxCollector::with_clock(move || base + *clock_offset.lock().expect("clock mutex"));
        collector.gpu_detector = Some(Box::new(move || {
            *detector_count.lock().expect("detector mutex") += 1;
            Some(Box::new(FakeBackend {
                counts: Arc::clone(&backend_counts),
                busy: HashMap::new(),
            }))
        }));

        let first = collector.collect().expect("cold collection");
        assert!(first.gpus.is_empty());
        assert_eq!(*detector_calls.lock().expect("detector mutex"), 0);

        *offset.lock().expect("clock mutex") = Duration::from_secs(61);
        let second = collector.collect().expect("hotplug collection");
        assert_eq!(*detector_calls.lock().expect("detector mutex"), 1);
        assert_eq!(counts.lock().expect("counts mutex").detect, 1);
        assert_eq!(counts.lock().expect("counts mutex").sample, 1);
        assert_eq!(second.gpus.len(), 1);
    }

    #[test]
    fn attach_gpu_maps_busy_by_pid_and_fills_absent_samples_with_none() {
        let mut snapshot = serde_json::from_value(json!({
            "timestamp": "2026-08-29T12:00:00Z",
            "identity": {
                "hostname": "fixture", "platform": "linux", "arch": "x86_64",
                "distro": "Fixture Linux", "kernel": "6.8.0",
                "runtime": { "kind": "Linux", "confidence": "high", "reason": "fixture" },
                "uptimeSeconds": 60
            },
            "cpu": { "usagePercent": 1.0, "cores": 2 },
            "memory": { "totalBytes": 100, "availableBytes": 50, "usedBytes": 50, "usedPercent": 50.0 },
            "swap": { "totalBytes": 0, "freeBytes": 0, "usedBytes": 0, "usedPercent": 0.0 },
            "load": { "one": 0.0, "five": 0.0, "fifteen": 0.0 },
            "pressure": { "cpu": {}, "memory": {}, "io": {} },
            "filesystems": [],
            "processes": [
                { "pid": 1, "command": "one", "cpuPercent": 0.0, "memoryPercent": 0.0, "rssBytes": 1 },
                { "pid": 2, "command": "two", "cpuPercent": 0.0, "memoryPercent": 0.0, "rssBytes": 1 }
            ]
        }))
        .expect("synthetic snapshot");
        let adapters = vec![
            GpuAdapter {
                id: "pci-0000:02:00.0".to_string(),
                vendor: GpuVendor::Amd,
                name: "first".to_string(),
                driver: "amdgpu".to_string(),
            },
            GpuAdapter {
                id: "pci-0000:06:00.0".to_string(),
                vendor: GpuVendor::Other(0x1a03),
                name: "second".to_string(),
                driver: "unknown".to_string(),
            },
        ];
        let samples = vec![GpuSample {
            adapter_id: adapters[0].id.clone(),
            busy_percent: Some(37.0),
            memory_used_bytes: Some(6_000_640),
            memory_total_bytes: Some(2_147_483_648),
            temperature_c: Some(44.0),
        }];
        attach_gpu(
            &mut snapshot,
            &adapters,
            &samples,
            &HashMap::from([(2, 12.5)]),
        );

        assert_eq!(snapshot.processes[0].gpu_percent, None);
        assert_eq!(snapshot.processes[1].gpu_percent, Some(12.5));
        assert_eq!(snapshot.gpus.len(), 2);
        assert_eq!(snapshot.gpus[0].busy_percent, Some(37.0));
        assert_eq!(snapshot.gpus[1].vendor, "0x1a03");
        assert_eq!(snapshot.gpus[1].busy_percent, None);
        assert_eq!(snapshot.gpus[1].memory_used_bytes, None);
        assert_eq!(snapshot.gpus[1].memory_total_bytes, None);
        assert_eq!(snapshot.gpus[1].temperature_c, None);
    }
}

#[cfg(test)]
mod thermal_tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };

    use super::LinuxCollector;
    use crate::{Collector, CollectorConfig};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("hexe-linux-thermal-{name}-{serial}"));
            fs::create_dir_all(&root).expect("create Linux thermal fixture root");
            Self { root }
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().expect("fixture file parent"))
                .expect("create fixture directory");
            fs::write(path, contents).expect("write fixture file");
        }

        fn root(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700));
            fs::remove_dir_all(&self.root).expect("remove Linux thermal fixture root");
        }
    }

    fn sheep_fixture() -> Fixture {
        let fixture = Fixture::new("sheep");
        fixture.write("hwmon0/name", "nvme\n");
        fixture.write("hwmon0/temp1_input", "52850\n");
        fixture.write("hwmon0/temp1_label", "Composite\n");
        fixture.write("hwmon1/name", "coretemp\n");
        for (number, label, input) in [
            (1, "Package id 0", "54000\n"),
            (2, "Core 0", "54000\n"),
            (3, "Core 1", "53000\n"),
            (4, "Core 2", "53000\n"),
            (5, "Core 3", "53000\n"),
        ] {
            fixture.write(&format!("hwmon1/temp{number}_input"), input);
            fixture.write(&format!("hwmon1/temp{number}_label"), label);
            fixture.write(&format!("hwmon1/temp{number}_max"), "105000\n");
            fixture.write(&format!("hwmon1/temp{number}_crit"), "105000\n");
        }
        fixture
    }

    #[test]
    fn disabled_never_touches_the_hwmon_tree() {
        let fixture = Fixture::new("disabled");
        let blocked = fixture.root().join("blocked");
        fs::create_dir_all(&blocked).expect("create blocked fixture path");
        let permission_variant =
            fs::set_permissions(fixture.root(), fs::Permissions::from_mode(0o000)).is_ok();

        let mut collector = LinuxCollector::with_clock(Instant::now);
        collector.thermal_root = if permission_variant {
            blocked
        } else {
            fixture.root().join("does-not-exist")
        };
        collector.configure(CollectorConfig {
            thermal_enabled: false,
            ..CollectorConfig::default()
        });

        let first = collector.collect().expect("first disabled collection");
        let second = collector.collect().expect("second disabled collection");
        assert!(first.sensors.is_empty());
        assert!(second.sensors.is_empty());
        assert_eq!(collector.thermal_scan_calls, 0);
        eprintln!(
            "disabled hwmon test variant: {}",
            if permission_variant {
                "permission-blocked tree"
            } else {
                "non-existent fallback path"
            }
        );
    }

    #[test]
    fn enabled_reads_the_fixture_tree() {
        let fixture = sheep_fixture();
        let mut collector = LinuxCollector::with_clock(Instant::now);
        collector.thermal_root = fixture.root().to_path_buf();
        collector.configure(CollectorConfig {
            thermal_enabled: true,
            ..CollectorConfig::default()
        });

        let snapshot = collector.collect().expect("enabled thermal collection");
        assert_eq!(snapshot.sensors.len(), 5);
    }
}
