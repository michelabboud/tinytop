use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{AssertSqlSafe, Row, SqlitePool};
use tinytop_store::{
    HistoryQuery, ProcessHistorySource, SqliteHistoryStore,
    maintenance::{LadderConfig, maintain_with_config},
};
use tinytop_types::{
    CpuSnapshot, CpuTimes, GpuSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

struct TempDatabase {
    dir: PathBuf,
    url: String,
}

impl TempDatabase {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time follows epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-gpu-history-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("fixture directory");
        Self {
            url: format!("sqlite://{}", dir.join("history.sqlite").display()),
            dir,
        }
    }

    async fn store(&self) -> SqliteHistoryStore {
        SqliteHistoryStore::connect(&self.url)
            .await
            .expect("fixture store")
    }

    async fn pool(&self) -> SqlitePool {
        SqlitePool::connect(&self.url)
            .await
            .expect("verification pool")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn gpu(id: &str, busy_percent: Option<f64>) -> GpuSnapshot {
    GpuSnapshot {
        id: id.to_string(),
        vendor: if id.ends_with("02:00.0") {
            "amd"
        } else {
            "intel"
        }
        .to_string(),
        name: format!("GPU {id}"),
        driver: if id.ends_with("02:00.0") {
            "amdgpu"
        } else {
            "i915"
        }
        .to_string(),
        busy_percent,
        memory_used_bytes: busy_percent.map(|_| 6_000_640),
        memory_total_bytes: busy_percent.map(|_| 2_147_483_648),
        temperature_c: busy_percent.map(|_| 44.0),
    }
}

fn snapshot(
    gpus: Vec<GpuSnapshot>,
    started_at: Option<&str>,
    gpu_percent: Option<f64>,
) -> SystemSnapshot {
    SystemSnapshot {
        timestamp: "2026-08-29T05:30:00Z".to_string(),
        filesystems_captured_at_ms: None,
        identity: IdentitySnapshot {
            hostname: "fixture-host".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Fixture Linux".to_string(),
            kernel: "6.8.0".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Linux,
                confidence: RuntimeConfidence::High,
                reason: "fixture".to_string(),
            },
            uptime_seconds: 123,
        },
        cpu: CpuSnapshot {
            usage_percent: 12.5,
            cores: 4,
            times: Some(CpuTimes::default()),
        },
        memory: MemorySnapshot {
            total_bytes: 1_000,
            available_bytes: 400,
            used_bytes: 600,
            used_percent: 60.0,
        },
        swap: SwapSnapshot {
            total_bytes: 100,
            free_bytes: 75,
            used_bytes: 25,
            used_percent: 25.0,
        },
        load: LoadSnapshot {
            one: 0.1,
            five: 0.2,
            fifteen: 0.3,
            runnable: Some(1),
            total_threads: Some(10),
            last_pid: Some(42),
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: Vec::new(),
        processes: vec![ProcessSnapshot {
            pid: 42,
            command: "fixture --work".to_string(),
            cpu_percent: 1.0,
            memory_percent: 2.0,
            rss_bytes: 3,
            parent_pid: Some(1),
            started_at: started_at.map(str::to_string),
            gpu_percent,
        }],
        gpus,
    }
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    let sql = match table {
        "gpu_adapters" => "SELECT COUNT(*) FROM gpu_adapters",
        "gpu_samples" => "SELECT COUNT(*) FROM gpu_samples",
        other => panic!("unsupported table {other}"),
    };
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("row count")
}

#[tokio::test]
async fn gpu_rows_are_written_per_adapter_per_tick_and_adapters_are_interned_once() {
    // Break caught: ticks overwrite adapter rows, duplicate identities, or skip a sample.
    let fixture = TempDatabase::new("rows");
    let store = fixture.store().await;
    for captured_at_ms in [1_000, 2_500, 4_000] {
        store
            .insert_snapshot(
                captured_at_ms,
                &snapshot(
                    vec![
                        gpu("pci-0000:02:00.0", Some(37.0)),
                        gpu("pci-0000:00:02.0", None),
                    ],
                    None,
                    None,
                ),
            )
            .await
            .expect("GPU tick");
    }
    let pool = fixture.pool().await;
    assert_eq!(count(&pool, "gpu_adapters").await, 2);
    assert_eq!(count(&pool, "gpu_samples").await, 6);
    let first_seen: Vec<i64> =
        sqlx::query_scalar("SELECT first_seen_ms FROM gpu_adapters ORDER BY stable_id")
            .fetch_all(&pool)
            .await
            .expect("first seen");
    assert_eq!(first_seen, [1_000, 1_000]);
}

#[tokio::test]
async fn adapter_ids_survive_a_reconnect() {
    // Break caught: the cache is not primed and reconnect creates a second dictionary row.
    let fixture = TempDatabase::new("reconnect");
    let store = fixture.store().await;
    store
        .insert_snapshot(
            1_000,
            &snapshot(vec![gpu("pci-0000:02:00.0", Some(1.0))], None, None),
        )
        .await
        .expect("first tick");
    let pool = fixture.pool().await;
    let before: i64 = sqlx::query_scalar(
        "SELECT adapter_id FROM gpu_adapters WHERE stable_id = 'pci-0000:02:00.0'",
    )
    .fetch_one(&pool)
    .await
    .expect("adapter id before reconnect");
    pool.close().await;
    store.close().await.expect("close first store");

    let store = fixture.store().await;
    store
        .insert_snapshot(
            2_500,
            &snapshot(vec![gpu("pci-0000:02:00.0", Some(2.0))], None, None),
        )
        .await
        .expect("second tick");
    let pool = fixture.pool().await;
    let after: i64 = sqlx::query_scalar(
        "SELECT adapter_id FROM gpu_adapters WHERE stable_id = 'pci-0000:02:00.0'",
    )
    .fetch_one(&pool)
    .await
    .expect("adapter id after reconnect");
    assert_eq!(after, before);
    assert_eq!(count(&pool, "gpu_adapters").await, 1);
}

#[tokio::test]
async fn last_seen_is_written_at_most_once_a_minute() {
    // Break caught: last_seen_ms dirties a page every fast tick or fails to advance after a minute.
    let fixture = TempDatabase::new("last-seen");
    let store = fixture.store().await;
    for captured_at_ms in [0, 1_500, 59_000] {
        store
            .insert_snapshot(
                captured_at_ms,
                &snapshot(vec![gpu("pci-0000:02:00.0", None)], None, None),
            )
            .await
            .expect("tick before minute");
    }
    let pool = fixture.pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT last_seen_ms FROM gpu_adapters")
            .fetch_one(&pool)
            .await
            .expect("last seen before minute"),
        0
    );
    store
        .insert_snapshot(
            61_000,
            &snapshot(vec![gpu("pci-0000:02:00.0", None)], None, None),
        )
        .await
        .expect("tick after minute");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT last_seen_ms FROM gpu_adapters")
            .fetch_one(&pool)
            .await
            .expect("last seen after minute"),
        61_000
    );
}

#[tokio::test]
async fn a_snapshot_without_gpus_writes_nothing() {
    // Break caught: an empty optional wire field creates placeholder adapter/sample rows.
    let fixture = TempDatabase::new("empty");
    let store = fixture.store().await;
    store
        .insert_snapshot(1_000, &snapshot(Vec::new(), None, None))
        .await
        .expect("GPU-less tick");
    let pool = fixture.pool().await;
    assert_eq!(count(&pool, "gpu_adapters").await, 0);
    assert_eq!(count(&pool, "gpu_samples").await, 0);
    let stats = store.stats().await.expect("stats");
    assert_eq!(stats.gpu_adapter_count, 0);
    assert_eq!(stats.gpu_sample_count, 0);
}

#[tokio::test]
async fn read_history_gpus_filters_by_adapter_and_orders_ascending() {
    // Break caught: the additive history query ignores its exact adapter filter or returns DESC rows.
    let fixture = TempDatabase::new("read");
    let store = fixture.store().await;
    for captured_at_ms in [3_000, 1_000, 2_000] {
        store
            .insert_snapshot(
                captured_at_ms,
                &snapshot(
                    vec![
                        gpu("pci-0000:02:00.0", Some(captured_at_ms as f64 / 1_000.0)),
                        gpu("pci-0000:00:02.0", None),
                    ],
                    None,
                    None,
                ),
            )
            .await
            .expect("GPU tick");
    }
    let rows = store
        .read_history_gpus(
            HistoryQuery {
                since_ms: Some(1_000),
                until_ms: Some(3_000),
                limit: Some(10),
            },
            Some("pci-0000:02:00.0"),
        )
        .await
        .expect("GPU history");
    assert_eq!(
        rows.iter()
            .map(|row| row.captured_at_ms)
            .collect::<Vec<_>>(),
        [1_000, 2_000, 3_000]
    );
    assert!(rows.iter().all(|row| row.id == "pci-0000:02:00.0"));
    assert_eq!(rows[0].busy_percent, Some(1.0));
    assert_eq!(rows[2].temperature_c, Some(44.0));
}

#[tokio::test]
async fn assembled_history_carries_gpus_and_process_gpu_percent() {
    // Break caught: typed assembly omits GPU rows or loses per-process GPU percent.
    let fixture = TempDatabase::new("assembly");
    let store = fixture.store().await;
    store
        .insert_snapshot(
            1_000,
            &snapshot(vec![gpu("pci-0000:02:00.0", Some(37.0))], None, Some(12.5)),
        )
        .await
        .expect("GPU tick");
    store
        .insert_snapshot(2_500, &snapshot(Vec::new(), None, None))
        .await
        .expect("GPU-less tick");

    let rows = store
        .read_history(HistoryQuery::default())
        .await
        .expect("assembled history");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].snapshot.gpus, [gpu("pci-0000:02:00.0", Some(37.0))]);
    assert_eq!(rows[0].snapshot.processes[0].gpu_percent, Some(12.5));
    assert!(rows[1].snapshot.gpus.is_empty());
    assert_eq!(rows[1].snapshot.processes[0].gpu_percent, None);
}

#[tokio::test]
async fn started_at_round_trips_through_ms_storage_on_both_tiers() {
    // Break caught: either process tier stores text/zero or read formatting changes whole-second RFC 3339.
    let fixture = TempDatabase::new("started-at");
    let store = fixture.store().await;
    let captured_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time follows epoch")
        .as_millis() as i64;
    store
        .insert_snapshot(
            captured_at_ms,
            &snapshot(Vec::new(), Some("2026-08-29T05:28:11Z"), None),
        )
        .await
        .expect("process tick");
    let pool = fixture.pool().await;
    for table in ["process_samples_fast", "process_samples"] {
        let value: Option<i64> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT started_at_ms FROM {table} WHERE captured_at_ms = ?"
        )))
        .bind(captured_at_ms)
        .fetch_one(&pool)
        .await
        .expect("stored started_at_ms");
        assert_eq!(value, Some(1_787_981_291_000), "{table}");
    }

    let assembled = store
        .read_history(HistoryQuery {
            since_ms: Some(captured_at_ms),
            until_ms: Some(captured_at_ms),
            limit: Some(1),
        })
        .await
        .expect("fast assembled history");
    assert_eq!(
        assembled[0].snapshot.processes[0].started_at.as_deref(),
        Some("2026-08-29T05:28:11Z")
    );

    let minute = store
        .read_history_processes(HistoryQuery {
            since_ms: None,
            until_ms: Some(captured_at_ms),
            limit: Some(1),
        })
        .await
        .expect("minute process history");
    assert_eq!(minute.source, ProcessHistorySource::Minute);
    assert_eq!(
        minute.captures[0].processes[0].started_at.as_deref(),
        Some("2026-08-29T05:28:11Z")
    );
}

#[tokio::test]
async fn an_unparsable_started_at_is_stored_as_null_and_the_row_is_kept() {
    // Break caught: invalid display-only text aborts a process transaction or is decoded as epoch zero.
    let fixture = TempDatabase::new("invalid-started-at");
    let store = fixture.store().await;
    store
        .insert_snapshot(1_000, &snapshot(Vec::new(), Some("-"), None))
        .await
        .expect("invalid startedAt tick");
    let pool = fixture.pool().await;
    for table in ["process_samples_fast", "process_samples"] {
        let row = sqlx::query(AssertSqlSafe(format!(
            "SELECT started_at_ms FROM {table} WHERE captured_at_ms = 1000"
        )))
        .fetch_one(&pool)
        .await
        .expect("process row retained");
        assert_eq!(row.get::<Option<i64>, _>("started_at_ms"), None, "{table}");
    }
}

#[tokio::test]
async fn gpu_rows_prune_at_the_l1_horizon() {
    // Break caught: raw metric pruning leaves GPU samples growing beyond L1.
    let fixture = TempDatabase::new("prune");
    let store = fixture.store().await;
    for captured_at_ms in [0, 100] {
        store
            .insert_snapshot(
                captured_at_ms,
                &snapshot(vec![gpu("pci-0000:02:00.0", None)], None, None),
            )
            .await
            .expect("GPU tick");
    }
    let config = LadderConfig {
        l1_keep_ms: 50,
        l2_keep_ms: 365 * DAY_MS,
        l3: None,
        l4: None,
        detail_interval_ms: MINUTE_MS,
        process_fast_keep_ms: DAY_MS,
        poll_interval_ms: 1_500,
    };
    let report = maintain_with_config(&store, &config, 100)
        .await
        .expect("maintenance");
    assert_eq!(report.gpu_rows, 1);
    let pool = fixture.pool().await;
    assert_eq!(count(&pool, "gpu_samples").await, 1);
}

#[tokio::test]
async fn stats_report_adapter_and_sample_counts() {
    // Break caught: db stats omit the new flattened counters or count the wrong table.
    let fixture = TempDatabase::new("stats");
    let store = fixture.store().await;
    store
        .insert_snapshot(
            1_000,
            &snapshot(
                vec![
                    gpu("pci-0000:02:00.0", Some(1.0)),
                    gpu("pci-0000:00:02.0", None),
                ],
                None,
                None,
            ),
        )
        .await
        .expect("GPU tick");
    let stats = store.stats().await.expect("stats");
    assert_eq!(stats.gpu_adapter_count, 2);
    assert_eq!(stats.gpu_sample_count, 2);
}
