use tinytop_store::{HistoryQuery, SqliteHistoryStore};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

#[tokio::test]
async fn sqlite_store_creates_missing_database_file() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tinytop-store-{stamp}"));
    std::fs::create_dir_all(&dir).expect("temp dir should be created");
    let db_path = dir.join("history.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());

    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should create a missing SQLite file");
    store
        .insert_snapshot(1_000, &snapshot("2026-06-24T12:00:01Z", 10.0))
        .await
        .expect("insert should work after file creation");

    assert!(db_path.exists());
    std::fs::remove_dir_all(dir).ok();
}

fn snapshot(timestamp: &str, cpu: f64) -> SystemSnapshot {
    SystemSnapshot {
        timestamp: timestamp.to_string(),
        identity: IdentitySnapshot {
            hostname: "devbox".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Ubuntu 24.04.2 LTS".to_string(),
            kernel: "6.8.0-52-generic".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Linux,
                confidence: RuntimeConfidence::High,
                reason: "fixture".to_string(),
            },
            uptime_seconds: 60,
        },
        cpu: CpuSnapshot {
            usage_percent: cpu,
            cores: 4,
            times: CpuTimes::default(),
        },
        memory: MemorySnapshot {
            total_bytes: 100,
            available_bytes: 40,
            used_bytes: 60,
            used_percent: 60.0,
        },
        swap: SwapSnapshot {
            total_bytes: 10,
            free_bytes: 5,
            used_bytes: 5,
            used_percent: 50.0,
        },
        load: LoadSnapshot {
            one: 1.0,
            five: 2.0,
            fifteen: 3.0,
            runnable: 1,
            total_threads: 2,
            last_pid: 3,
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: vec![FilesystemSnapshot {
            filesystem: "/dev/sda1".to_string(),
            fs_type: "ext4".to_string(),
            size_bytes: 100,
            used_bytes: 50,
            available_bytes: 50,
            used_percent: 50.0,
            mount: "/".to_string(),
            inode_used_percent: Some(10.0),
            inode_used: Some(1),
            inode_total: Some(10),
        }],
        processes: vec![ProcessSnapshot {
            pid: 42,
            command: "tinytop".to_string(),
            cpu_percent: 1.0,
            memory_percent: 2.0,
            rss_bytes: 3,
        }],
    }
}

#[tokio::test]
async fn sqlite_store_inserts_latest_and_reads_history_oldest_first() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store");

    store
        .insert_snapshot(1_000, &snapshot("2026-06-24T12:00:01Z", 10.0))
        .await
        .expect("insert first");
    store
        .insert_snapshot(2_000, &snapshot("2026-06-24T12:00:02Z", 20.0))
        .await
        .expect("insert second");

    let latest = store.latest_snapshot().await.expect("latest").expect("row");
    assert_eq!(latest.captured_at_ms, 2_000);
    assert_eq!(latest.snapshot.cpu.usage_percent, 20.0);

    let stats = store.stats().await.expect("stats");
    assert_eq!(stats.sample_count, 2);
    assert_eq!(stats.oldest_captured_at_ms, Some(1_000));
    assert_eq!(stats.newest_captured_at_ms, Some(2_000));

    let integrity = store.integrity_check().await.expect("integrity check");
    assert_eq!(integrity, "ok");

    let history = store
        .read_history(HistoryQuery {
            since_ms: Some(0),
            until_ms: None,
            limit: Some(10),
        })
        .await
        .expect("history");

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].captured_at_ms, 1_000);
    assert_eq!(history[1].captured_at_ms, 2_000);
}

#[tokio::test]
async fn sqlite_store_upserts_duplicate_sample_timestamps() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store");

    store
        .insert_snapshot(1_000, &snapshot("2026-06-24T12:00:01Z", 10.0))
        .await
        .expect("insert first");
    store
        .insert_snapshot(1_000, &snapshot("2026-06-24T12:00:01Z", 99.0))
        .await
        .expect("upsert");

    let history = store
        .read_history(HistoryQuery {
            since_ms: Some(0),
            until_ms: None,
            limit: Some(10),
        })
        .await
        .expect("history");

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].snapshot.cpu.usage_percent, 99.0);
}
