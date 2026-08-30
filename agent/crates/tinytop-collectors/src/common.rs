use std::{
    env,
    ffi::OsStr,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, ProcessRefreshKind, ProcessesToUpdate, System,
    UpdateKind,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tinytop_types::{
    CpuSnapshot, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot, PressureGroup,
    PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection, RuntimeKind,
    SwapSnapshot, SystemSnapshot,
};

use crate::{CollectorConfig, CollectorError, CollectorResult};

struct SysinfoSlowCache {
    taken_at: Instant,
    captured_at_ms: i64,
    filesystems: Vec<FilesystemSnapshot>,
    hostname: String,
    distro: String,
    kernel: String,
}

pub struct SysinfoCollector {
    system: System,
    initialized_cpu_usage: bool,
    config: CollectorConfig,
    slow_cache: Option<SysinfoSlowCache>,
}

impl Default for SysinfoCollector {
    fn default() -> Self {
        Self {
            system: System::new(),
            initialized_cpu_usage: false,
            config: CollectorConfig::default(),
            slow_cache: None,
        }
    }
}

impl SysinfoCollector {
    pub fn configure(&mut self, config: CollectorConfig) {
        if self.config != config {
            self.config = config;
        }
    }

    pub fn collect(
        &mut self,
        runtime_kind: RuntimeKind,
        platform: &'static str,
        default_name: &'static str,
        reason: &'static str,
    ) -> CollectorResult<SystemSnapshot> {
        self.refresh();
        let now = Instant::now();
        let slow_due = self.slow_cache.as_ref().is_none_or(|cache| {
            now.duration_since(cache.taken_at) >= self.config.filesystems_interval
        });
        if slow_due {
            let disks = Disks::new_with_refreshed_list_specifics(
                DiskRefreshKind::nothing().with_kind().with_storage(),
            );
            self.slow_cache = Some(SysinfoSlowCache {
                taken_at: now,
                captured_at_ms: now_unix_ms()?,
                filesystems: native_filesystems(&disks),
                hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
                distro: System::long_os_version()
                    .or_else(System::name)
                    .unwrap_or_else(|| default_name.to_string()),
                kernel: System::kernel_version().unwrap_or_else(|| platform.to_string()),
            });
        }
        let slow = self
            .slow_cache
            .as_ref()
            .expect("the first collection always populates the slow cache");
        let processes = native_processes(&self.system, self.config.top_process_count);
        let load = System::load_average();
        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let total_swap = self.system.total_swap();
        let used_swap = self.system.used_swap();

        Ok(SystemSnapshot {
            timestamp: now_rfc3339()?,
            filesystems_captured_at_ms: Some(slow.captured_at_ms),
            identity: IdentitySnapshot {
                hostname: slow.hostname.clone(),
                platform: platform.to_string(),
                arch: env::consts::ARCH.to_string(),
                distro: slow.distro.clone(),
                kernel: slow.kernel.clone(),
                runtime: RuntimeDetection {
                    kind: runtime_kind,
                    confidence: RuntimeConfidence::Medium,
                    reason: reason.to_string(),
                },
                uptime_seconds: System::uptime(),
            },
            cpu: CpuSnapshot {
                usage_percent: round_percent(self.system.global_cpu_usage() as f64),
                cores: self.system.cpus().len().max(1),
                // sysinfo exposes aggregate usage but no platform CPU tick breakdown.
                times: None,
            },
            memory: MemorySnapshot {
                total_bytes: total_memory,
                available_bytes: self.system.available_memory(),
                used_bytes: used_memory,
                used_percent: percent(used_memory, total_memory),
            },
            swap: SwapSnapshot {
                total_bytes: total_swap,
                free_bytes: total_swap.saturating_sub(used_swap),
                used_bytes: used_swap,
                used_percent: percent(used_swap, total_swap),
            },
            load: LoadSnapshot {
                one: load.one,
                five: load.five,
                fifteen: load.fifteen,
                runnable: None,
                total_threads: None,
                last_pid: None,
            },
            pressure: PressureGroup {
                cpu: PressureSnapshot::default(),
                memory: PressureSnapshot::default(),
                io: PressureSnapshot::default(),
            },
            filesystems: slow.filesystems.clone(),
            processes,
            gpus: Vec::new(),
        })
    }

    fn refresh(&mut self) {
        self.system
            .refresh_cpu_list(CpuRefreshKind::nothing().with_cpu_usage());
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .without_tasks(),
        );
        self.system.refresh_cpu_usage();
        if !self.initialized_cpu_usage {
            thread::sleep(Duration::from_millis(120));
            self.system.refresh_cpu_usage();
            self.initialized_cpu_usage = true;
        }
    }
}

fn native_filesystems(disks: &Disks) -> Vec<FilesystemSnapshot> {
    disks
        .list()
        .iter()
        .map(|disk| {
            let size = disk.total_space();
            let available = disk.available_space();
            let used = size.saturating_sub(available);
            FilesystemSnapshot {
                filesystem: os_str_to_string(disk.name()),
                fs_type: os_str_to_string(disk.file_system()),
                size_bytes: size,
                used_bytes: used,
                available_bytes: available,
                used_percent: percent(used, size),
                mount: path_to_string(disk.mount_point()),
                inode_used_percent: None,
                inode_used: None,
                inode_total: None,
            }
        })
        .collect()
}

fn native_processes(system: &System, top_process_count: usize) -> Vec<ProcessSnapshot> {
    // Validated settings are at least one; keep that invariant for direct callers.
    let top_process_count = top_process_count.max(1);
    let total_memory = system.total_memory();
    let mut processes = system
        .processes()
        .values()
        .map(|process| {
            let memory_percent = if total_memory == 0 {
                0.0
            } else {
                round_percent((process.memory() as f64 / total_memory as f64) * 100.0)
            };
            ProcessSnapshot {
                pid: process.pid().as_u32(),
                command: process_command(process),
                cpu_percent: round_percent(process.cpu_usage() as f64),
                memory_percent,
                rss_bytes: process.memory(),
                parent_pid: process.parent().map(|pid| pid.as_u32()),
                started_at: process_started_at(process),
                gpu_percent: None,
            }
        })
        .collect::<Vec<_>>();
    processes.sort_by(|left, right| {
        right
            .cpu_percent
            .partial_cmp(&left.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    processes.truncate(top_process_count);
    processes
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

fn now_rfc3339() -> CollectorResult<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| CollectorError::parse("format timestamp", error.to_string()))
}

fn now_unix_ms() -> CollectorResult<i64> {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| CollectorError::parse("read timestamp", "milliseconds exceed i64"))
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    round_percent((used as f64 / total as f64) * 100.0)
}

fn round_percent(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn os_str_to_string(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

fn path_to_string(value: &Path) -> String {
    value.to_string_lossy().into_owned()
}
