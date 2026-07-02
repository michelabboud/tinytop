use std::{collections::HashMap, env, ffi::OsStr, path::Path, thread, time::Duration};

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

use crate::{Collector, CollectorError, CollectorResult};

#[derive(Debug, Clone)]
pub struct LinuxSnapshotSources {
    pub timestamp: String,
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

pub struct LinuxCollector {
    previous_proc_stat_text: Option<String>,
    system: System,
}

impl Default for LinuxCollector {
    fn default() -> Self {
        Self {
            previous_proc_stat_text: None,
            system: System::new(),
        }
    }
}

impl LinuxCollector {
    pub fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        if env::consts::OS != "linux" {
            return Err(CollectorError::UnsupportedPlatform {
                platform: env::consts::OS,
            });
        }

        let sources = collect_sources(&mut self.system, self.previous_proc_stat_text.as_deref())?;
        self.previous_proc_stat_text = Some(sources.current_proc_stat_text.clone());
        build_linux_snapshot_from_sources(sources)
    }
}

impl Collector for LinuxCollector {
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
            times: current_cpu,
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
    })
}

pub fn collect_sources(
    system: &mut System,
    previous_proc_stat_text: Option<&str>,
) -> CollectorResult<LinuxSnapshotSources> {
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
    let disks = Disks::new_with_refreshed_list_specifics(
        DiskRefreshKind::nothing().with_kind().with_storage(),
    );

    Ok(LinuxSnapshotSources {
        timestamp: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| CollectorError::parse("format timestamp", error.to_string()))?,
        hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
        platform: "linux".to_string(),
        arch: env::consts::ARCH.to_string(),
        cpu_count: system.cpus().len().max(1),
        os_release_text: sysinfo_os_release_text(),
        proc_version: System::kernel_version().unwrap_or_default(),
        kernel_release: System::kernel_version().unwrap_or_else(|| env::consts::OS.to_string()),
        wsl_distro_name: env::var("WSL_DISTRO_NAME").ok(),
        wsl_interop: env::var("WSL_INTEROP").ok(),
        uptime_text: procfs_uptime_text()?,
        meminfo_text: procfs_meminfo_text(&meminfo),
        loadavg_text: procfs_loadavg_text()?,
        previous_proc_stat_text: first_proc_stat,
        current_proc_stat_text: current_proc_stat,
        cpu_pressure_text: cpu_pressure_text(),
        memory_pressure_text: memory_pressure_text(),
        io_pressure_text: io_pressure_text(),
        df_blocks_text: sysinfo_df_blocks_text(&disks),
        df_inodes_text: statvfs_inodes_text(&disks),
        ps_text: sysinfo_process_text(system, meminfo.mem_total),
    })
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
        runnable: parse_u64(thread_counts.first().copied().unwrap_or("0"), "runnable")?,
        total_threads: parse_u64(
            thread_counts.get(1).copied().unwrap_or("0"),
            "total threads",
        )?,
        last_pid: parse_u64(parts[4], "last pid")?,
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
        .take(10)
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

fn sysinfo_process_text(system: &System, total_memory: u64) -> String {
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
        .take(10)
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
