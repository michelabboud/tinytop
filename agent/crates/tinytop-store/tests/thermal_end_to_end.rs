use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tinytop_store::SqliteHistoryStore;
use tinytop_types::{
    CpuSnapshot, CpuTimes, IdentitySnapshot, LoadSnapshot, MemorySnapshot, PressureGroup,
    PressureSnapshot, RuntimeConfidence, RuntimeDetection, RuntimeKind, SwapSnapshot,
    SystemSnapshot,
};

// Reuse the production, OS-independent thermal collector directly. The store crate cannot add a
// dev-dependency on the collectors crate in this fix lane, and this keeps the proof dependency-free.
mod thermal_collector {
    include!("../../tinytop-collectors/src/thermal.rs");
}

struct TempDatabase {
    dir: PathBuf,
    url: String,
}

impl TempDatabase {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time follows epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "hexe-sensorless-end-to-end-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("fixture directory");
        Self {
            url: format!("sqlite://{}", dir.join("history.sqlite").display()),
            dir,
        }
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn snapshot() -> SystemSnapshot {
    SystemSnapshot {
        timestamp: "2026-08-31T08:00:00Z".to_string(),
        filesystems_captured_at_ms: None,
        identity: IdentitySnapshot {
            hostname: "sensorless-fixture".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Fixture Linux".to_string(),
            kernel: "6.8.0".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Wsl,
                confidence: RuntimeConfidence::High,
                reason: "fixture".to_string(),
            },
            uptime_seconds: 1,
        },
        cpu: CpuSnapshot {
            usage_percent: 0.0,
            cores: 1,
            times: Some(CpuTimes::default()),
        },
        memory: MemorySnapshot {
            total_bytes: 1,
            available_bytes: 1,
            used_bytes: 0,
            used_percent: 0.0,
        },
        swap: SwapSnapshot {
            total_bytes: 0,
            free_bytes: 0,
            used_bytes: 0,
            used_percent: 0.0,
        },
        load: LoadSnapshot {
            one: 0.0,
            five: 0.0,
            fifteen: 0.0,
            runnable: None,
            total_threads: None,
            last_pid: None,
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: Vec::new(),
        processes: Vec::new(),
        gpus: Vec::new(),
        sensors: Vec::new(),
    }
}

#[tokio::test]
async fn sensorless_host_end_to_end_writes_nothing() {
    // Break caught: a missing WSL2 hwmon root leaks a wire field or placeholder DB rows.
    let fixture = TempDatabase::new();
    let missing_hwmon_root = fixture.dir.join("missing-hwmon");
    assert!(!missing_hwmon_root.exists());

    let thermal_enabled = true;
    let mut collected = snapshot();
    if thermal_enabled {
        let scan = thermal_collector::scan(&missing_hwmon_root, &[]);
        collected.sensors = thermal_collector::read_values(&missing_hwmon_root, &scan.sensors);
    }

    assert!(collected.sensors.is_empty());
    let wire = serde_json::to_value(&collected).expect("snapshot wire value");
    assert!(wire.get("sensors").is_none());

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh store");
    store
        .insert_snapshot(1_000, &collected)
        .await
        .expect("sensorless snapshot insert");
    let pool = SqlitePool::connect(&fixture.url)
        .await
        .expect("verification pool");
    let sensor_dim_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sensor_dim")
        .fetch_one(&pool)
        .await
        .expect("sensor dimension count");
    let sensor_sample_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sensor_samples")
        .fetch_one(&pool)
        .await
        .expect("sensor sample count");
    assert_eq!(sensor_dim_rows, 0);
    assert_eq!(sensor_sample_rows, 0);
}
