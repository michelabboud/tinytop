use tinytop_store::HistorySample;
use tinytop_types::{
    CpuSnapshot, CpuTimes, IdentitySnapshot, LoadSnapshot, MemorySnapshot, PressureGroup,
    PressureSnapshot, RuntimeConfidence, RuntimeDetection, RuntimeKind, SwapSnapshot,
    SystemSnapshot,
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
}

fn minimal_snapshot() -> SystemSnapshot {
    SystemSnapshot {
        timestamp: "2026-06-25T00:00:00Z".to_string(),
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
            times: CpuTimes::default(),
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
            runnable: 1,
            total_threads: 2,
            last_pid: 3,
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: vec![],
        processes: vec![],
    }
}
