use tinytop_store::HistorySample;
use tinytop_types::{
    CpuSnapshot, CpuTimes, GpuSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

#[test]
fn history_sample_serializes_with_dashboard_field_names() {
    let sample = HistorySample {
        captured_at_ms: 1_772_000_000_000,
        snapshot: minimal_snapshot(),
    };

    let value = serde_json::to_value(&sample).expect("history sample should serialize");

    assert_eq!(value["capturedAtMs"], 1_772_000_000_000_i64);
    assert!(value.get("captured_at_ms").is_none());
    assert!(value.get("snapshot").is_some());
    assert!(value["snapshot"].get("gpus").is_none());
}

#[test]
fn history_sample_with_a_gpu_serializes_with_dashboard_field_names() {
    // Break caught: assembled GPU and per-process values use Rust field names or disappear.
    let mut snapshot = minimal_snapshot();
    snapshot.gpus.push(GpuSnapshot {
        id: "pci-0000:02:00.0".to_string(),
        vendor: "amd".to_string(),
        name: "0x1002:0x6810".to_string(),
        driver: "amdgpu".to_string(),
        busy_percent: Some(37.0),
        memory_used_bytes: Some(6_000_640),
        memory_total_bytes: Some(2_147_483_648),
        temperature_c: Some(44.0),
    });
    snapshot.processes.push(ProcessSnapshot {
        pid: 42,
        command: "fixture".to_string(),
        cpu_percent: 1.0,
        memory_percent: 2.0,
        rss_bytes: 3,
        parent_pid: None,
        started_at: None,
        gpu_percent: Some(12.5),
    });

    let value = serde_json::to_value(HistorySample {
        captured_at_ms: 1_772_000_000_000,
        snapshot,
    })
    .expect("GPU history sample should serialize");

    assert_eq!(value["snapshot"]["gpus"][0]["busyPercent"], 37.0);
    assert_eq!(value["snapshot"]["gpus"][0]["memoryUsedBytes"], 6_000_640);
    assert_eq!(
        value["snapshot"]["gpus"][0]["memoryTotalBytes"],
        2_147_483_648_u64
    );
    assert_eq!(value["snapshot"]["gpus"][0]["temperatureC"], 44.0);
    assert_eq!(value["snapshot"]["processes"][0]["gpuPercent"], 12.5);
}

fn minimal_snapshot() -> SystemSnapshot {
    SystemSnapshot {
        timestamp: "2026-06-25T00:00:00Z".to_string(),
        filesystems_captured_at_ms: None,
        identity: IdentitySnapshot {
            hostname: "host".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Ubuntu".to_string(),
            kernel: "6.0.0".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Linux,
                confidence: RuntimeConfidence::High,
                reason: "test".to_string(),
            },
            uptime_seconds: 42,
        },
        cpu: CpuSnapshot {
            usage_percent: 1.0,
            cores: 4,
            times: Some(CpuTimes::default()),
        },
        memory: MemorySnapshot {
            total_bytes: 100,
            available_bytes: 75,
            used_bytes: 25,
            used_percent: 25.0,
        },
        swap: SwapSnapshot {
            total_bytes: 100,
            free_bytes: 90,
            used_bytes: 10,
            used_percent: 10.0,
        },
        load: LoadSnapshot {
            one: 0.1,
            five: 0.2,
            fifteen: 0.3,
            runnable: Some(1),
            total_threads: Some(2),
            last_pid: Some(3),
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: vec![],
        processes: vec![],
        gpus: Vec::new(),
    }
}
