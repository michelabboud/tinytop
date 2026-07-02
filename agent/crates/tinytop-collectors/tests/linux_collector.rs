#![cfg(all(feature = "linux-collector", target_os = "linux"))]

use tinytop_collectors::linux::{
    LinuxCollector, build_linux_snapshot_from_sources, calculate_cpu_usage,
    decode_proc_mount_escape, detect_linux_runtime, parse_df_blocks, parse_loadavg, parse_meminfo,
    parse_pressure, parse_proc_stat,
};
use tinytop_types::{RuntimeConfidence, RuntimeKind};

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
    assert_eq!(load.runnable, 7);

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

    let mut collector = LinuxCollector::default();
    let snapshot = collector.collect().expect("live linux snapshot");

    assert!(!snapshot.identity.hostname.is_empty());
    assert_eq!(snapshot.identity.platform, "linux");
    assert!(snapshot.cpu.cores > 0);
    assert!(snapshot.memory.total_bytes > 0);
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
