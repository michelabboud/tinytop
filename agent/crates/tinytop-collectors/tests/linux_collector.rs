#![cfg(all(feature = "linux-collector", target_os = "linux"))]

use std::{
    collections::HashSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tinytop_collectors::{
    Collector, CollectorConfig, DEFAULT_TOP_PROCESS_COUNT,
    linux::{
        LinuxCollector, SWAP_READ_BUDGET_PER_TICK, SwapScanStats,
        build_linux_snapshot_from_sources, calculate_cpu_usage, decode_proc_mount_escape,
        detect_linux_runtime, parse_df_blocks, parse_loadavg, parse_meminfo, parse_pressure,
        parse_proc_stat, parse_vm_swap, scan_process_swap,
    },
};
use tinytop_types::{ProcessSnapshot, RuntimeConfidence, RuntimeKind};

fn linux_fixture() -> tinytop_collectors::linux::LinuxSnapshotSources {
    tinytop_collectors::linux::LinuxSnapshotSources {
        timestamp: "2026-06-24T12:00:00Z".to_string(),
        hostname: "devbox".to_string(),
        platform: "linux".to_string(),
        arch: "x86_64".to_string(),
        cpu_count: 4,
        os_release_text: r#"PRETTY_NAME="Ubuntu 24.04.2 LTS""#.to_string(),
        proc_version: "Linux version 5.15.167.4-microsoft-standard-WSL2".to_string(),
        kernel_release: "5.15.167.4-microsoft-standard-WSL2".to_string(),
        wsl_distro_name: Some("Ubuntu-24.04".to_string()),
        wsl_interop: Some("/run/WSL/123_interop".to_string()),
        uptime_text: "3661.42 1234.00".to_string(),
        meminfo_text: r#"
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
Cached:          3072000 kB
SwapTotal:       4194304 kB
SwapFree:        1048576 kB
"#.to_string(),
        loadavg_text: "1.20 2.30 3.40 5/678 9012".to_string(),
        previous_proc_stat_text: "cpu  100 0 100 800 0 0 0 0 0 0".to_string(),
        current_proc_stat_text: "cpu  150 0 150 900 0 0 0 0 0 0".to_string(),
        cpu_pressure_text: "some avg10=2.00 avg60=1.00 avg300=0.50 total=100\n".to_string(),
        memory_pressure_text:
            "some avg10=3.00 avg60=2.00 avg300=1.00 total=200\nfull avg10=0.30 avg60=0.20 avg300=0.10 total=20\n"
                .to_string(),
        io_pressure_text:
            "some avg10=4.00 avg60=3.00 avg300=2.00 total=300\nfull avg10=0.40 avg60=0.30 avg300=0.20 total=30\n"
                .to_string(),
        df_blocks_text: r#"Filesystem     Type  1-blocks     Used Available Use% Mounted on
/dev/sdd       ext4  1000 800 200 80% /
tmpfs          tmpfs 500 25 475 5% /run
"#.to_string(),
        df_inodes_text: r#"Filesystem     Type  Inodes IUsed IFree IUse% Mounted on
/dev/sdd       ext4  100 20 80 20% /
tmpfs          tmpfs 50 1 49 2% /run
"#.to_string(),
        ps_text: r#"101 12.5 1.2 120117 bun
202 3.1 5.4 445313 postgres
"#.to_string(),
        filesystems_captured_at_ms: 1_777_777_777_777,
    }
}

#[test]
fn parses_linux_sources_into_the_existing_snapshot_contract() {
    let snapshot = build_linux_snapshot_from_sources(linux_fixture()).expect("snapshot");

    assert_eq!(snapshot.identity.hostname, "devbox");
    assert_eq!(snapshot.identity.distro, "Ubuntu 24.04.2 LTS");
    assert_eq!(snapshot.identity.runtime.kind, RuntimeKind::Wsl);
    assert_eq!(
        snapshot.identity.runtime.confidence,
        RuntimeConfidence::High
    );
    assert_eq!(snapshot.cpu.usage_percent, 50.0);
    assert_eq!(snapshot.memory.used_percent, 50.0);
    assert_eq!(snapshot.swap.used_percent, 75.0);
    assert_eq!(snapshot.load.one, 1.2);
    assert_eq!(snapshot.filesystems[0].mount, "/");
    assert_eq!(snapshot.filesystems[0].inode_used_percent, Some(20.0));
    assert_eq!(
        snapshot.pressure.memory.full.as_ref().expect("full").avg10,
        0.3
    );
    assert_eq!(snapshot.processes[0].command, "bun");
    assert!(snapshot.gpus.is_empty());

    let json = serde_json::to_value(snapshot).expect("json");
    assert_eq!(json["cpu"]["usagePercent"], 50.0);
    assert_eq!(json["memory"]["totalBytes"], 16_777_216_000u64);
    assert_eq!(json["filesystems"][0]["inodeUsedPercent"], 20.0);
}

#[test]
fn parser_helpers_match_the_bun_collector_math() {
    let mem = parse_meminfo(&linux_fixture().meminfo_text).expect("meminfo");
    assert_eq!(mem.used_percent, 50.0);
    assert_eq!(mem.swap_used_percent, 75.0);

    let load = parse_loadavg("1.23 2.34 3.45 7/890 12345").expect("loadavg");
    assert_eq!(load.runnable, Some(7));
    assert_eq!(load.total_threads, Some(890));
    assert_eq!(load.last_pid, Some(12_345));

    let previous = parse_proc_stat("cpu  100 0 100 800 0 0 0 0 0 0").expect("previous cpu");
    let current = parse_proc_stat("cpu  150 0 150 900 0 0 0 0 0 0").expect("current cpu");
    assert_eq!(calculate_cpu_usage(&previous, &current), 50.0);

    let pressure =
        parse_pressure("some avg10=1.23 avg60=4.56 avg300=7.89 total=123456\n").expect("pressure");
    assert_eq!(pressure.some.expect("some").avg300, 7.89);

    let filesystems = parse_df_blocks(&linux_fixture().df_blocks_text).expect("df");
    assert_eq!(filesystems[0].used_percent, 80.0);
}

#[test]
fn detects_wsl_and_real_linux_conservatively() {
    let wsl = detect_linux_runtime(
        "5.15.167.4-microsoft-standard-WSL2",
        "Linux version 5.15.167.4-microsoft-standard-WSL2",
        None,
        None,
    );
    assert_eq!(wsl.kind, RuntimeKind::Wsl);
    assert_eq!(wsl.confidence, RuntimeConfidence::High);

    let linux = detect_linux_runtime(
        "6.8.0-52-generic",
        "Linux version 6.8.0-52-generic",
        None,
        None,
    );
    assert_eq!(linux.kind, RuntimeKind::Linux);
    assert_eq!(linux.confidence, RuntimeConfidence::High);
}

#[test]
fn decodes_proc_mount_octal_escapes_from_wsl_disk_names() {
    assert_eq!(
        decode_proc_mount_escape(r"C:\134Program\040Files\134Docker"),
        r"C:\Program Files\Docker"
    );
}

#[test]
fn live_linux_collector_returns_a_real_snapshot_on_linux_hosts() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let mut collector = LinuxCollector::with_clock(Instant::now);
    let snapshot = collector.collect().expect("live linux snapshot");

    assert!(!snapshot.identity.hostname.is_empty());
    assert_eq!(snapshot.identity.platform, "linux");
    assert!(snapshot.cpu.cores > 0);
    assert!(snapshot.memory.total_bytes > 0);
}

#[test]
#[ignore = "rule-8 live scan-cost instrument; run explicitly on GPU acceptance hosts"]
fn live_gpu_scan_cost_on_this_host() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let mut collector = LinuxCollector::default();
    collector.collect().expect("first live GPU collection");
    std::thread::sleep(Duration::from_secs(1));
    let snapshot = collector.collect().expect("second live GPU collection");
    println!("gpu adapter count: {}", snapshot.gpus.len());
    println!(
        "gpu snapshot: {}",
        serde_json::to_string(&snapshot.gpus).expect("serialize live GPU snapshot")
    );
    match collector.last_gpu_scan() {
        Some(stats) => println!(
            "gpu scan stats: duration={:?} pids_scanned={} pids_denied={} clients={}",
            stats.duration, stats.pids_scanned, stats.pids_denied, stats.clients
        ),
        None => println!("gpu scan stats: none (no detected adapter)"),
    }
}

#[test]
fn merges_statvfs_inode_text_into_filesystem_snapshots() {
    // The df_inodes_text produced by statvfs collection must flow through the
    // existing parse/merge path and populate inode fields keyed by mount (M1).
    let mut sources = linux_fixture();
    sources.df_inodes_text = "Filesystem Type Inodes IUsed IFree IUse% Mounted on\n\
        /dev/sdd ext4 262144 52428 209716 20% /\n\
        tmpfs tmpfs 2048000 12 2047988 1% /run"
        .to_string();

    let snapshot = build_linux_snapshot_from_sources(sources).expect("snapshot");
    let root = snapshot
        .filesystems
        .iter()
        .find(|fs| fs.mount == "/")
        .expect("root filesystem");
    assert_eq!(root.inode_total, Some(262_144));
    assert_eq!(root.inode_used, Some(52_428));
    assert_eq!(root.inode_used_percent, Some(20.0));
}

#[test]
fn collect_sources_populates_inode_data_from_statvfs_on_linux() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let mut system = sysinfo::System::new();
    let sources = tinytop_collectors::linux::collect_sources(&mut system, None)
        .expect("collect_sources should succeed on linux");

    assert!(
        !sources.df_inodes_text.trim().is_empty(),
        "statvfs inode collection should populate df_inodes_text on a linux host"
    );

    let snapshot = build_linux_snapshot_from_sources(sources).expect("snapshot builds");
    assert!(
        snapshot
            .filesystems
            .iter()
            .any(|filesystem| filesystem.inode_total.is_some()),
        "at least one filesystem should carry inode totals collected via statvfs"
    );
}

#[test]
fn linux_collector_does_not_shell_out_for_host_metrics() {
    let source = include_str!("../src/linux.rs");
    for forbidden in [
        "std::process",
        "Command::new",
        "run_text_optional",
        "[\"df\"",
        "[\"ps\"",
        "[\"uname\"",
    ] {
        assert!(
            !source.contains(forbidden),
            "linux collector source must not contain external command path: {forbidden}"
        );
    }
}

fn clocked_collector() -> (LinuxCollector, Arc<Mutex<Duration>>) {
    let base = Instant::now();
    let offset = Arc::new(Mutex::new(Duration::ZERO));
    let clock_offset = Arc::clone(&offset);
    let collector =
        LinuxCollector::with_clock(move || base + *clock_offset.lock().expect("test clock mutex"));
    (collector, offset)
}

#[test]
fn filesystems_are_served_from_cache_between_slow_ticks() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let (mut collector, offset) = clocked_collector();
    let first = collector.collect().expect("first collection");
    let first_captured_at = first.filesystems_captured_at_ms;
    assert_eq!(collector.slow_enumerations(), 1);

    *offset.lock().expect("test clock mutex") = Duration::from_secs(30);
    let second = collector.collect().expect("second collection");

    assert_eq!(second.filesystems, first.filesystems);
    assert_eq!(second.filesystems_captured_at_ms, first_captured_at);
    assert_eq!(collector.slow_enumerations(), 1);
    assert_ne!(second.timestamp, first.timestamp);
}

#[test]
fn slow_tick_re_enumerates_after_the_interval() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let (mut collector, offset) = clocked_collector();
    let first = collector.collect().expect("first collection");
    let first_captured_at = first.filesystems_captured_at_ms;

    *offset.lock().expect("test clock mutex") = Duration::from_secs(61);
    let second = collector.collect().expect("second collection");

    assert_eq!(collector.slow_enumerations(), 2);
    assert!(second.filesystems_captured_at_ms > first_captured_at);
}

#[test]
fn configure_changes_the_interval_without_resetting_the_cache() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let (mut collector, offset) = clocked_collector();
    collector.collect().expect("first collection");
    collector.configure(CollectorConfig::default());

    *offset.lock().expect("test clock mutex") = Duration::from_secs(30);
    collector.collect().expect("cached collection");
    assert_eq!(collector.slow_enumerations(), 1);

    collector.configure(CollectorConfig {
        filesystems_interval: Duration::from_secs(10),
        ..CollectorConfig::default()
    });
    collector.collect().expect("shortened-interval collection");
    assert_eq!(collector.slow_enumerations(), 2);
}

/// What every live sample must look like for a list length of `count`,
/// whatever the host is running: the CPU list first and in rank order, then
/// the memory-only rows in rank order, each process once, and two lists of
/// the same length. Returns that length.
fn assert_sample_shape(processes: &[ProcessSnapshot], count: usize) -> usize {
    let cpu_ranked = processes
        .iter()
        .take_while(|process| process.cpu_rank.is_some())
        .count();
    for (index, process) in processes.iter().enumerate() {
        if index < cpu_ranked {
            assert_eq!(process.cpu_rank, u32::try_from(index).ok(), "row {index}");
        } else {
            assert_eq!(process.cpu_rank, None, "row {index} is after the CPU list");
            assert!(
                process.memory_rank.is_some(),
                "row {index} is in neither list"
            );
        }
    }
    let tail_ranks = processes[cpu_ranked..]
        .iter()
        .filter_map(|process| process.memory_rank)
        .collect::<Vec<_>>();
    assert!(tail_ranks.is_sorted(), "memory-only rows: {tail_ranks:?}");

    let mut memory_ranks = processes
        .iter()
        .filter_map(|process| process.memory_rank)
        .collect::<Vec<_>>();
    memory_ranks.sort_unstable();
    let expected = (0..u32::try_from(memory_ranks.len()).expect("rank count")).collect::<Vec<_>>();
    assert_eq!(memory_ranks, expected, "memory ranks are 0..len, each once");
    assert_eq!(memory_ranks.len(), cpu_ranked, "both lists are one length");

    let pids = processes
        .iter()
        .map(|process| process.pid)
        .collect::<HashSet<_>>();
    assert_eq!(pids.len(), processes.len(), "each process appears once");
    assert!(
        cpu_ranked <= count,
        "{cpu_ranked} CPU rows for count {count}"
    );
    assert!(
        (cpu_ranked..=2 * cpu_ranked).contains(&processes.len()),
        "{} rows for two lists of {cpu_ranked}",
        processes.len()
    );
    cpu_ranked
}

#[test]
fn an_unconfigured_collector_keeps_twelve_processes() {
    assert_eq!(DEFAULT_TOP_PROCESS_COUNT, 12);
    assert_eq!(
        CollectorConfig::default().top_process_count,
        DEFAULT_TOP_PROCESS_COUNT
    );
    if std::env::consts::OS != "linux" {
        return;
    }

    // The widest setting tells us how many processes this host can show, so
    // the default is checked exactly instead of with a `<=` that eight passes.
    // The count is the length of each list (CPU and memory), not of the
    // sample, which holds the union of the two.
    let mut collector = LinuxCollector::with_clock(Instant::now);
    collector.configure(CollectorConfig {
        top_process_count: 50,
        ..CollectorConfig::default()
    });
    let visible = assert_sample_shape(&collector.collect().expect("widest list").processes, 50);

    let mut unconfigured = LinuxCollector::with_clock(Instant::now);
    let kept = assert_sample_shape(
        &unconfigured.collect().expect("default list").processes,
        DEFAULT_TOP_PROCESS_COUNT,
    );
    if visible >= DEFAULT_TOP_PROCESS_COUNT {
        assert_eq!(kept, DEFAULT_TOP_PROCESS_COUNT);
    } else {
        // A containment lane with a tiny process table: every process fits.
        assert!(kept <= DEFAULT_TOP_PROCESS_COUNT);
        eprintln!("host exposes only {visible} process(es); default checked as an upper bound");
    }
}

#[test]
fn top_process_count_is_honoured() {
    if std::env::consts::OS != "linux" {
        return;
    }

    // The count bounds each of the two lists; the sample is their union.
    let mut collector = LinuxCollector::with_clock(Instant::now);
    collector.configure(CollectorConfig {
        top_process_count: 1,
        ..CollectorConfig::default()
    });
    let one = collector.collect().expect("one process per list").processes;
    assert_eq!(assert_sample_shape(&one, 1), 1);
    assert!((1..=2).contains(&one.len()), "{} rows", one.len());

    for count in [8, 50] {
        collector.configure(CollectorConfig {
            top_process_count: count,
            ..CollectorConfig::default()
        });
        let processes = collector.collect().expect("configured count").processes;
        assert!(assert_sample_shape(&processes, count) <= count);
        assert!(processes.len() <= 2 * count);
    }
}

fn tab_line(columns: [&str; 10]) -> String {
    columns.join("\t")
}

#[test]
fn process_text_carries_swap_and_both_ranks_into_the_snapshot() {
    let mut sources = linux_fixture();
    sources.ps_text = [
        // In both lists.
        tab_line([
            "101",
            "12.5",
            "1.2",
            "120117",
            "1",
            "2026-06-24T11:00:00Z",
            "0",
            "0",
            "1",
            "bun run dev",
        ]),
        // CPU only; swap unknown.
        tab_line([
            "202", "3.1", "0.1", "512", "-", "-", "-", "1", "-", "kworker",
        ]),
        // Memory only: swapped out, tiny resident set. A command may hold a tab.
        tab_line([
            "303",
            "0.0",
            "0.6",
            "104857",
            "101",
            "-",
            "2726297",
            "-",
            "0",
            "mai-core\t--serve",
        ]),
    ]
    .join("\n");

    let processes = build_linux_snapshot_from_sources(sources)
        .expect("snapshot")
        .processes;
    assert_eq!(processes.len(), 3);

    assert_eq!(processes[0].pid, 101);
    assert_eq!(processes[0].command, "bun run dev");
    assert_eq!(processes[0].rss_bytes, 120_117 * 1024);
    assert_eq!(processes[0].parent_pid, Some(1));
    assert_eq!(
        processes[0].started_at.as_deref(),
        Some("2026-06-24T11:00:00Z")
    );
    assert_eq!(
        processes[0].swap_bytes,
        Some(0),
        "a known zero is not unknown"
    );
    assert_eq!(processes[0].cpu_rank, Some(0));
    assert_eq!(processes[0].memory_rank, Some(1));

    assert_eq!(processes[1].swap_bytes, None);
    assert_eq!(processes[1].cpu_rank, Some(1));
    assert_eq!(processes[1].memory_rank, None);
    assert_eq!(processes[1].parent_pid, None);

    assert_eq!(processes[2].swap_bytes, Some(2_726_297 * 1024));
    assert_eq!(processes[2].cpu_rank, None);
    assert_eq!(processes[2].memory_rank, Some(0));
    assert_eq!(processes[2].command, "mai-core\t--serve");
    assert_sample_shape(&processes, 2);

    let json = serde_json::to_value(&processes).expect("json");
    assert_eq!(json[0]["swapBytes"], 0);
    assert_eq!(json[0]["cpuRank"], 0);
    assert_eq!(json[0]["memoryRank"], 1);
    assert!(json[1].get("swapBytes").is_none());
    assert!(json[1].get("memoryRank").is_none());
    assert_eq!(json[2]["swapBytes"], 2_791_728_128_u64);
    assert!(json[2].get("cpuRank").is_none());
}

#[test]
fn whitespace_process_lines_carry_no_swap_and_no_ranks() {
    // The `ps`-shaped fixture format has no column for them: unknown, not 0.
    let processes = build_linux_snapshot_from_sources(linux_fixture())
        .expect("snapshot")
        .processes;
    assert_eq!(processes.len(), 2);
    for process in &processes {
        assert_eq!(process.swap_bytes, None);
        assert_eq!(process.cpu_rank, None);
        assert_eq!(process.memory_rank, None);
    }
    let json = serde_json::to_value(&processes[0]).expect("json");
    assert_eq!(
        json,
        serde_json::json!({
            "pid": 101, "command": "bun", "cpuPercent": 12.5,
            "memoryPercent": 1.2, "rssBytes": 120_117 * 1024
        })
    );
}

#[test]
fn a_malformed_process_line_is_dropped_and_a_malformed_column_is_unknown() {
    let mut sources = linux_fixture();
    sources.ps_text = [
        // Nine columns (a line cut short): dropped, never shifted.
        ["1", "1.0", "0.1", "10", "-", "-", "0", "0", "0"].join("\t"),
        // The seven-column shape this text had before swap and ranks: dropped.
        ["2", "1.0", "0.1", "10", "-", "-", "old-format"].join("\t"),
        // A pid that is not a number: dropped.
        tab_line(["x", "1.0", "0.1", "10", "-", "-", "0", "0", "0", "bad-pid"]),
        // Unparsable optional columns: the row survives, the values are unknown.
        tab_line([
            "4", "1.0", "0.1", "10", "?", "-", "lots", "-1", "1.5", "kept",
        ]),
        // A swap figure too large for bytes saturates instead of wrapping.
        tab_line([
            "5",
            "1.0",
            "0.1",
            "10",
            "-",
            "-",
            "18446744073709551615",
            "-",
            "0",
            "huge",
        ]),
    ]
    .join("\n");

    let processes = build_linux_snapshot_from_sources(sources)
        .expect("a bad process line never fails the sample")
        .processes;
    assert_eq!(
        processes.iter().map(|p| p.pid).collect::<Vec<_>>(),
        vec![4, 5]
    );
    assert_eq!(processes[0].command, "kept");
    assert_eq!(processes[0].parent_pid, None);
    assert_eq!(processes[0].swap_bytes, None);
    assert_eq!(processes[0].cpu_rank, None);
    assert_eq!(processes[0].memory_rank, None);
    assert_eq!(processes[1].swap_bytes, Some(u64::MAX));
    assert_eq!(processes[1].memory_rank, Some(0));
}

/// A `/proc/<pid>/status` as Linux 6.x writes it for a user process, trimmed
/// to the lines around `VmSwap`.
fn status_text(vm_swap_line: &str) -> String {
    format!(
        "Name:\tmai-core\nUmask:\t0022\nState:\tS (sleeping)\nTgid:\t4242\nPid:\t4242\n\
         Groups:\t4 24 27 30 46 100 1000 \nVmPeak:\t 5242880 kB\nVmSize:\t 4194304 kB\n\
         VmRSS:\t  104857 kB\nRssAnon:\t   90000 kB\nVmData:\t 3000000 kB\nVmPTE:\t    7000 kB\n\
         {vm_swap_line}HugetlbPages:\t       0 kB\nCoreDumping:\t0\nThreads:\t17\n\
         voluntary_ctxt_switches:\t12\n"
    )
}

/// A kernel thread's status: no `Vm*` lines at all.
const KERNEL_THREAD_STATUS: &str = "Name:\tkworker/0:1\nUmask:\t0000\nState:\tI (idle)\nTgid:\t17\nPid:\t17\nPPid:\t2\nThreads:\t1\nSigQ:\t0/385911\nvoluntary_ctxt_switches:\t9\n";

#[test]
fn vm_swap_is_read_in_bytes_from_a_status_text() {
    let status = status_text("VmSwap:\t 2726297 kB\n");
    assert_eq!(parse_vm_swap(status.as_bytes()), Some(2_726_297 * 1024));
    assert_eq!(
        parse_vm_swap(status_text("VmSwap:\t       0 kB\n").as_bytes()),
        Some(0),
        "a process with nothing in swap is a known zero"
    );
    // The process's own name is arbitrary bytes; it must not hide the figure.
    let mut odd_name = b"Name:\t\xff\xfe\x80\n".to_vec();
    odd_name.extend_from_slice(b"VmSwap:\t      12 kB\n");
    assert_eq!(parse_vm_swap(&odd_name), Some(12 * 1024));
}

#[test]
fn a_missing_or_malformed_vm_swap_line_is_unknown() {
    assert_eq!(parse_vm_swap(KERNEL_THREAD_STATUS.as_bytes()), None);
    assert_eq!(parse_vm_swap(status_text("").as_bytes()), None);
    assert_eq!(parse_vm_swap(b""), None);
    for malformed in [
        "VmSwap:\n",                              // no value
        "VmSwap:\t kB\n",                         // unit, no number
        "VmSwap:\t 12\n",                         // no unit: cannot tell the scale
        "VmSwap:\t 12 MB\n",                      // a unit the kernel never writes
        "VmSwap:\t -12 kB\n",                     // negative
        "VmSwap:\t 1.5 kB\n",                     // fractional
        "VmSwap:\t twelve kB\n",                  // not a number
        "VmSwap:\t 12 kB extra\n",                // trailing field
        "VmSwap:\t 18446744073709551615 kB\n",    // would overflow in bytes
        "VmSwap:\t 99999999999999999999999 kB\n", // does not fit at all
        "VmSwapped:\t 12 kB\n",                   // a different key
        " VmSwap:\t 12 kB\n",                     // not at the start of the line
    ] {
        assert_eq!(
            parse_vm_swap(status_text(malformed).as_bytes()),
            None,
            "{malformed:?}"
        );
    }
    // Another key that merely mentions it is not the line.
    assert_eq!(parse_vm_swap(b"Name:\tVmSwap: 5 kB\n"), None);
}

static NEXT_PROC_FIXTURE: AtomicU64 = AtomicU64::new(0);

/// A throwaway `/proc`-shaped tree, built the way the thermal tests build
/// their `hwmon` tree.
struct ProcFixture {
    root: PathBuf,
}

impl ProcFixture {
    fn new(name: &str) -> Self {
        let serial = NEXT_PROC_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "tinytop-proc-swap-{name}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create proc fixture root");
        Self { root }
    }

    fn status(&self, pid: u32, contents: &str) -> PathBuf {
        let directory = self.root.join(pid.to_string());
        fs::create_dir_all(&directory).expect("create fixture pid directory");
        let path = directory.join("status");
        fs::write(&path, contents).expect("write fixture status");
        path
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for ProcFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove proc fixture root");
    }
}

#[test]
fn a_swap_scan_reads_each_process_and_tolerates_every_per_process_failure() {
    let fixture = ProcFixture::new("scan");
    fixture.status(100, &status_text("VmSwap:\t 2726297 kB\n"));
    fixture.status(200, &status_text("VmSwap:\t       0 kB\n"));
    // A kernel thread: the file is there, the line is not.
    fixture.status(300, KERNEL_THREAD_STATUS);
    // A malformed line.
    fixture.status(400, &status_text("VmSwap:\t lots kB\n"));
    // Pid 500 exited before the read: no directory at all.
    // Pid 600 exited between the open and the read: the open succeeds and the
    // read fails. A directory in place of the file fails the same way.
    fs::create_dir_all(fixture.root().join("600/status")).expect("create unreadable status");
    // Pid 700 belongs to someone else: the open is refused.
    let denied = fixture.status(700, &status_text("VmSwap:\t      64 kB\n"));
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o000)).expect("deny fixture status");
    // Root ignores file modes, so the refusal can only be shown unprivileged.
    let refusal_enforced = fs::read(&denied).is_err();

    let (swap, stats) = scan_process_swap(fixture.root(), [100, 200, 300, 400, 500, 600, 700]);

    assert_eq!(swap.get(&100), Some(&(2_726_297 * 1024)));
    assert_eq!(swap.get(&200), Some(&0));
    for unknown in [300, 400, 500, 600] {
        assert_eq!(swap.get(&unknown), None, "pid {unknown}");
    }
    assert_eq!(stats.pids_scanned, 7);
    if refusal_enforced {
        assert_eq!(swap.get(&700), None, "a refused open is unknown");
        assert_eq!(swap.len(), 2);
        assert_eq!(stats.pids_unknown, 5);
    } else {
        assert_eq!(swap.get(&700), Some(&(64 * 1024)));
        assert_eq!(stats.pids_unknown, 4);
        eprintln!("running as root: the permission-denied variant was not exercised");
    }
}

#[test]
fn a_status_larger_than_one_read_is_read_until_the_swap_line_is_whole() {
    // One read asks for 4096 bytes. `Groups:` precedes `VmSwap:` and is as
    // long as the process has groups, so the line can sit in a later read or
    // straddle two of them.
    const READ_BYTES: usize = 4096;
    let fixture = ProcFixture::new("large");
    let swap_line = "VmSwap:\t 2726297 kB\n";
    let tail = "HugetlbPages:\t       0 kB\nThreads:\t17\n";
    let padded = |prefix_bytes: usize| {
        let head = "Name:\tmany-groups\nGroups:\t";
        let mut text = String::from(head);
        text.push_str(&"7 ".repeat((prefix_bytes - head.len() - 1) / 2));
        while text.len() < prefix_bytes - 1 {
            text.push(' ');
        }
        text.push('\n');
        assert_eq!(text.len(), prefix_bytes);
        text.push_str(swap_line);
        text.push_str(tail);
        text
    };

    // Wholly inside the third read.
    fixture.status(1, &padded(2 * READ_BYTES + 100));
    // Break caught: judging a read that stops inside the number would report
    // a smaller figure, or none. Each of these splits the line across two
    // reads: after `VmSw`, inside the number, and before the unit.
    fixture.status(2, &padded(READ_BYTES - 4));
    fixture.status(3, &padded(READ_BYTES - 12));
    fixture.status(4, &padded(READ_BYTES - 17));
    // The line ends exactly at the end of the first read.
    fixture.status(5, &padded(READ_BYTES - swap_line.len()));
    // No newline after the last line of the file: judged at end of file.
    fixture.status(6, "Name:\tno-newline\nVmSwap:\t      64 kB");
    // A long file with no swap line at all is read to its end and is unknown.
    fixture.status(7, &padded(3 * READ_BYTES).replace("VmSwap", "VmSwop"));

    // The read buffer grows to fit the largest file of a pass and stays that
    // size, so each straddling file gets a pass of its own: after a larger
    // file, one read would return it whole and the split would never happen.
    for pid in 1..=5 {
        let (swap, stats) = scan_process_swap(fixture.root(), [pid]);
        assert_eq!(swap.get(&pid), Some(&(2_726_297 * 1024)), "pid {pid}");
        assert_eq!((stats.pids_scanned, stats.pids_unknown), (1, 0));
    }
    // And in one pass, smallest first and largest first.
    for pids in [[5, 4, 3, 2, 1, 6, 7], [7, 1, 2, 3, 4, 5, 6]] {
        let (swap, stats) = scan_process_swap(fixture.root(), pids);
        for pid in 1..=5 {
            assert_eq!(swap.get(&pid), Some(&(2_726_297 * 1024)), "pid {pid}");
        }
        assert_eq!(swap.get(&6), Some(&(64 * 1024)));
        assert_eq!(swap.get(&7), None);
        assert_eq!((stats.pids_scanned, stats.pids_unknown), (7, 1));
    }

    // A short status read after a long one must not see the long one's bytes
    // still sitting in the shared buffer.
    fixture.status(8, &status_text("VmSwap:\t       0 kB\n"));
    fixture.status(9, KERNEL_THREAD_STATUS);
    let (swap, _) = scan_process_swap(fixture.root(), [1, 8, 9]);
    assert_eq!(swap.get(&8), Some(&0));
    assert_eq!(
        swap.get(&9),
        None,
        "stale bytes from pid 1 leaked into pid 9"
    );
}

#[test]
fn a_swap_scan_of_a_missing_tree_or_of_nothing_is_empty_not_an_error() {
    let fixture = ProcFixture::new("absent");
    let (swap, stats) = scan_process_swap(&fixture.root().join("not-mounted"), [1, 2, 3]);
    assert!(swap.is_empty());
    assert_eq!((stats.pids_scanned, stats.pids_unknown), (3, 3));

    let (swap, stats) = scan_process_swap(fixture.root(), []);
    assert!(swap.is_empty());
    assert_eq!((stats.pids_scanned, stats.pids_unknown), (0, 0));
}

#[test]
fn the_budget_verdict_follows_the_measured_duration() {
    let at_budget = SwapScanStats {
        duration: SWAP_READ_BUDGET_PER_TICK,
        ..SwapScanStats::default()
    };
    assert!(at_budget.within_budget());
    let over = SwapScanStats {
        duration: SWAP_READ_BUDGET_PER_TICK + Duration::from_nanos(1),
        ..SwapScanStats::default()
    };
    assert!(!over.within_budget());
}

#[test]
fn a_live_sample_ranks_both_lists_and_reads_swap() {
    if std::env::consts::OS != "linux" {
        return;
    }

    let mut collector = LinuxCollector::with_clock(Instant::now);
    assert_eq!(collector.last_swap_scan(), None);
    collector.configure(CollectorConfig {
        top_process_count: 5,
        ..CollectorConfig::default()
    });
    let snapshot = collector.collect().expect("live linux snapshot");
    assert_sample_shape(&snapshot.processes, 5);

    let scan = collector
        .last_swap_scan()
        .expect("the collection scanned swap");
    assert!(scan.pids_scanned >= snapshot.processes.len());
    assert!(scan.pids_unknown <= scan.pids_scanned);

    // This test process is a user process the collector may read: its swap is
    // a known number (usually zero), never unknown.
    let own_pid = std::process::id();
    let (swap, own_scan) = scan_process_swap(Path::new("/proc"), [own_pid]);
    assert!(swap.contains_key(&own_pid), "own VmSwap should be readable");
    assert_eq!((own_scan.pids_scanned, own_scan.pids_unknown), (1, 0));
}

#[test]
#[ignore = "live cost instrument for the per-process swap read; timing is only meaningful run alone"]
fn live_swap_read_cost_on_this_host() {
    if std::env::consts::OS != "linux" {
        return;
    }

    const TICKS: usize = 25;
    let mut collector = LinuxCollector::with_clock(Instant::now);
    let mut scans = Vec::with_capacity(TICKS);
    let mut ticks = Vec::with_capacity(TICKS);
    for _ in 0..TICKS {
        let started = Instant::now();
        collector.collect().expect("live collection");
        ticks.push(started.elapsed());
        scans.push(collector.last_swap_scan().expect("swap scan stats"));
    }
    // The first tick carries the collector's 120 ms CPU warm-up sleep.
    ticks.remove(0);
    ticks.sort_unstable();
    let last = scans[TICKS - 1];
    let mut durations = scans.iter().map(|scan| scan.duration).collect::<Vec<_>>();
    durations.sort_unstable();
    let median = durations[TICKS / 2];
    let worst = durations[TICKS - 1];

    println!(
        "swap read per tick over {TICKS} ticks: min={:?} median={median:?} max={worst:?}",
        durations[0]
    );
    println!(
        "processes scanned={} swap unknown={} ({:?} per process at the median)",
        last.pids_scanned,
        last.pids_unknown,
        median / u32::try_from(last.pids_scanned.max(1)).expect("process count")
    );
    println!(
        "whole collection tick (swap read included), ticks 2..{TICKS}: median={:?} max={:?}",
        ticks[ticks.len() / 2],
        ticks[ticks.len() - 1]
    );
    println!("budget SWAP_READ_BUDGET_PER_TICK={SWAP_READ_BUDGET_PER_TICK:?}");
    assert!(
        median <= SWAP_READ_BUDGET_PER_TICK,
        "median swap read {median:?} exceeds the {SWAP_READ_BUDGET_PER_TICK:?} per-tick budget"
    );
}
