use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tinytop_store::{
    DashboardSettings, SqliteHistoryStore,
    ladder::{Stat, Tier, TierBucket},
    maintenance::{LadderConfig, maintain_with_config},
};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;

struct TempDatabase {
    dir: PathBuf,
    url: String,
}

impl TempDatabase {
    fn new(prefix: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-ladder-{prefix}-{}-{stamp}",
            std::process::id()
        ));
        assert!(dir.starts_with(std::env::temp_dir()));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("history.sqlite");
        let url = format!("sqlite://{}", path.display());
        Self { dir, url }
    }

    async fn store(&self) -> SqliteHistoryStore {
        SqliteHistoryStore::connect(&self.url)
            .await
            .expect("fixture store should connect")
    }

    async fn pool(&self) -> SqlitePool {
        SqlitePool::connect(&self.url)
            .await
            .expect("fixture verification pool should connect")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn config() -> LadderConfig {
    LadderConfig {
        l1_keep_ms: 365 * DAY_MS,
        l2_keep_ms: 365 * DAY_MS,
        l3: Some(365 * DAY_MS),
        l4: Some(730 * DAY_MS),
        snapshot_json_keep_ms: 365 * DAY_MS,
        detail_interval_ms: MINUTE_MS,
        poll_interval_ms: 1_500,
    }
}

fn bucket(start: i64, resolution_ms: i64, count: i64, value: f64) -> TierBucket {
    let stat = Stat {
        avg: value,
        min: value,
        max: value,
    };
    TierBucket {
        bucket_start_ms: start,
        first_captured_at_ms: start,
        newest_captured_at_ms: start + resolution_ms - 1,
        sample_count: count,
        cpu: stat,
        memory: stat,
        swap: stat,
        load: stat,
        root_used: Some(stat),
    }
}

#[tokio::test]
async fn decimation_regression_completed_minute_keeps_its_sample_count() {
    // Break caught: pruning L1 rebuilds the cutoff minute from its surviving
    // tail, corrupting a completed L2 aggregate that previously represented 40 rows.
    let fixture = TempDatabase::new("decimation");
    let store = fixture.store().await;
    for minute in 0..3_i64 {
        for sample_index in 0..40_i64 {
            let captured_at_ms = minute * MINUTE_MS + sample_index * 1_500;
            store
                .insert_snapshot(
                    captured_at_ms,
                    &snapshot(captured_at_ms, 10.0 + minute as f64),
                )
                .await
                .expect("fixture sample should insert");
        }
    }
    let before = store
        .read_tier_buckets(Tier::L2, 0, 2 * MINUTE_MS)
        .await
        .expect("completed minute buckets before maintenance");
    assert_eq!(before.len(), 2);
    assert_eq!(before[0].sample_count, 40);
    assert_eq!(before[1].sample_count, 40);

    let mut settings = config();
    settings.l1_keep_ms = 90_000;
    maintain_with_config(&store, &settings, 3 * MINUTE_MS + 5_000)
        .await
        .expect("maintenance should run");

    let after = store
        .read_tier_buckets(Tier::L2, 0, 2 * MINUTE_MS)
        .await
        .expect("completed minute buckets after maintenance");
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].sample_count, 40);
    assert_eq!(
        after[1].sample_count, 40,
        "bucket 1 sample_count collapsed from 40 to {}",
        after[1].sample_count
    );
    assert_eq!(after[0].cpu.avg, before[0].cpu.avg);
    assert_eq!(after[1].cpu.avg, before[1].cpu.avg);
}

#[tokio::test]
async fn insert_rebuilds_its_minute_with_min_and_max() {
    // Break caught: the insert path updates averages and maxima but leaves the
    // v1 minimum columns NULL or computes the wrong extrema.
    let fixture = TempDatabase::new("minute-extrema");
    let store = fixture.store().await;
    for (captured_at_ms, cpu) in [(1_000, 10.0), (2_000, 50.0), (3_000, 30.0)] {
        store
            .insert_snapshot(captured_at_ms, &snapshot(captured_at_ms, cpu))
            .await
            .expect("fixture sample should insert");
    }

    let rows = store
        .read_tier_buckets(Tier::L2, 0, MINUTE_MS)
        .await
        .expect("minute bucket should read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sample_count, 3);
    assert_eq!(rows[0].cpu.min, 10.0);
    assert_eq!(rows[0].cpu.max, 50.0);
    assert_eq!(rows[0].cpu.avg, 30.0);
}

#[tokio::test]
async fn l2_rows_survive_their_horizon_until_l3_has_folded_them() {
    // Break caught: expired L2 rows are deleted before the first L3 promotion
    // establishes a watermark proving the same data was saved coarsely.
    let fixture = TempDatabase::new("promote-before-prune");
    let store = fixture.store().await;
    for minute in 0..20_i64 {
        store
            .upsert_tier_bucket(
                Tier::L2,
                &bucket(minute * MINUTE_MS, MINUTE_MS, 40, minute as f64),
            )
            .await
            .expect("L2 fixture bucket should upsert");
    }
    let mut settings = config();
    settings.l2_keep_ms = 5 * MINUTE_MS;
    settings.l4 = None;
    let now_ms = 20 * MINUTE_MS;

    let first = maintain_with_config(&store, &settings, now_ms)
        .await
        .expect("first maintenance pass should run");
    assert_eq!(first.pruned[1], 0);
    assert_eq!(first.promoted_l3, 3);
    assert_eq!(
        store
            .read_tier_buckets(Tier::L2, 0, 20 * MINUTE_MS)
            .await
            .expect("L2 rows after first pass")
            .len(),
        20
    );
    assert_eq!(
        store
            .read_tier_buckets(Tier::L3, 0, 20 * MINUTE_MS)
            .await
            .expect("L3 rows after first pass")
            .len(),
        3
    );
    assert_eq!(
        store
            .history_state_get::<i64>("l3FoldedUntilMs")
            .await
            .expect("L3 watermark should read"),
        Some(15 * MINUTE_MS)
    );

    let second = maintain_with_config(&store, &settings, now_ms)
        .await
        .expect("second maintenance pass should run");
    assert_eq!(second.pruned[1], 15);
    assert_eq!(
        store
            .read_tier_buckets(Tier::L2, 0, 20 * MINUTE_MS)
            .await
            .expect("L2 rows after second pass")
            .len(),
        5
    );
}

#[tokio::test]
async fn promotion_is_bounded_per_call() {
    // Break caught: a long outage makes one maintenance tick fold an unbounded
    // backlog, or the 50-bucket cap prevents later ticks from converging.
    let fixture = TempDatabase::new("bounded-promotion");
    let store = fixture.store().await;
    for minute in 0..2_880_i64 {
        store
            .upsert_tier_bucket(
                Tier::L2,
                &bucket(minute * MINUTE_MS, MINUTE_MS, 1, minute as f64),
            )
            .await
            .expect("L2 fixture bucket should upsert");
    }
    let mut settings = config();
    settings.l4 = None;
    let now_ms = 2 * DAY_MS + 3_000;
    let mut promoted = 0;
    let mut calls = 0;
    loop {
        let report = maintain_with_config(&store, &settings, now_ms)
            .await
            .expect("maintenance pass should run");
        assert!(report.promoted_l3 <= 50);
        promoted += report.promoted_l3;
        calls += 1;
        if report.promoted_l3 == 0 {
            break;
        }
        assert!(calls < 20, "bounded promotion should converge");
    }

    assert_eq!(promoted, 576);
    assert_eq!(
        store
            .read_tier_buckets(Tier::L3, 0, 2 * DAY_MS)
            .await
            .expect("all L3 buckets should read")
            .len(),
        576
    );
}

#[tokio::test]
async fn late_write_refolds_ancestors() {
    // Break caught: a late raw write repairs its L2 minute but leaves already
    // promoted L3 and L4 ancestors frozen with stale counts and averages.
    let fixture = TempDatabase::new("late-write");
    let store = fixture.store().await;
    for minute in 0..60_i64 {
        let captured_at_ms = minute * MINUTE_MS;
        store
            .insert_snapshot(captured_at_ms, &snapshot(captured_at_ms, 10.0))
            .await
            .expect("fixture sample should insert");
    }
    let settings = config();
    let now_ms = 60 * MINUTE_MS + 3_000;
    maintain_with_config(&store, &settings, now_ms)
        .await
        .expect("first promotion pass should run");
    let before_l3 = store
        .read_tier_buckets(Tier::L3, 0, 5 * MINUTE_MS)
        .await
        .expect("L3 ancestor before late write")
        .remove(0);
    let before_l4 = store
        .read_tier_buckets(Tier::L4, 0, 60 * MINUTE_MS)
        .await
        .expect("L4 ancestor before late write")
        .remove(0);

    store
        .insert_snapshot(30_000, &snapshot(30_000, 90.0))
        .await
        .expect("late sample should insert and refold ancestors");

    let after_l3 = store
        .read_tier_buckets(Tier::L3, 0, 5 * MINUTE_MS)
        .await
        .expect("L3 ancestor after late write")
        .remove(0);
    let after_l4 = store
        .read_tier_buckets(Tier::L4, 0, 60 * MINUTE_MS)
        .await
        .expect("L4 ancestor after late write")
        .remove(0);
    assert_eq!(after_l3.sample_count, before_l3.sample_count + 1);
    assert_eq!(after_l4.sample_count, before_l4.sample_count + 1);
    assert!(after_l3.cpu.avg > before_l3.cpu.avg);
    assert!(after_l4.cpu.avg > before_l4.cpu.avg);
}

#[tokio::test]
async fn json_is_stripped_outside_the_keep_window() {
    // Break caught: typed L1 rows outside the JSON window retain their large
    // snapshot payload, or recent raw API rows lose JSON at the inclusive boundary.
    let fixture = TempDatabase::new("json-strip");
    let store = fixture.store().await;
    for index in 0..200_i64 {
        let captured_at_ms = index * 30_000;
        store
            .insert_snapshot(captured_at_ms, &snapshot(captured_at_ms, 10.0))
            .await
            .expect("fixture sample should insert");
    }
    let mut settings = config();
    settings.snapshot_json_keep_ms = 60 * MINUTE_MS;
    let now_ms = 100 * MINUTE_MS;

    let report = maintain_with_config(&store, &settings, now_ms)
        .await
        .expect("maintenance should strip JSON");
    assert_eq!(report.json_stripped, 80);
    let pool = fixture.pool().await;
    let json_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples WHERE snapshot_json IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("JSON-bearing row count");
    assert_eq!(json_rows, 120);
    pool.close().await;
}

#[tokio::test]
async fn l4_forever_never_prunes_l4() {
    // Break caught: keepDays=0 is interpreted as an immediate cutoff instead
    // of the explicit forever sentinel.
    let fixture = TempDatabase::new("l4-forever");
    let store = fixture.store().await;
    store
        .upsert_tier_bucket(Tier::L4, &bucket(0, 60 * MINUTE_MS, 2_400, 10.0))
        .await
        .expect("ancient L4 fixture bucket should upsert");
    let mut settings = config();
    settings.l3 = None;
    settings.l4 = Some(0);

    let report = maintain_with_config(&store, &settings, 10 * 365 * DAY_MS)
        .await
        .expect("maintenance should preserve forever L4");
    assert_eq!(report.expired_l4, 0);
    assert_eq!(
        store
            .read_tier_buckets(Tier::L4, 0, 60 * MINUTE_MS)
            .await
            .expect("forever L4 rows should read")
            .len(),
        1
    );
}

#[tokio::test]
async fn disabled_tier_is_neither_written_nor_pruned() {
    // Break caught: disabling L3 mutates its dormant table, or prevents L4 from
    // folding directly from the nearest enabled finer tier (L2).
    let fixture = TempDatabase::new("disabled-tier");
    let store = fixture.store().await;
    store
        .upsert_tier_bucket(Tier::L3, &bucket(0, 5 * MINUTE_MS, 999, 99.0))
        .await
        .expect("dormant L3 fixture bucket should upsert");
    store
        .history_state_set("l3FoldedUntilMs", &(60 * MINUTE_MS), 0)
        .await
        .expect("fixture should represent a formerly enabled L3 watermark");
    for minute in 0..60_i64 {
        store
            .upsert_tier_bucket(Tier::L2, &bucket(minute * MINUTE_MS, MINUTE_MS, 40, 10.0))
            .await
            .expect("L2 fixture bucket should upsert");
    }
    let mut settings = config();
    settings.l3 = None;

    maintain_with_config(&store, &settings, 60 * MINUTE_MS + 3_000)
        .await
        .expect("maintenance should fold L4 directly from L2");

    let l3 = store
        .read_tier_buckets(Tier::L3, 0, 5 * MINUTE_MS)
        .await
        .expect("dormant L3 row should read");
    assert_eq!(l3.len(), 1);
    assert_eq!(l3[0].sample_count, 999);
    let l4 = store
        .read_tier_buckets(Tier::L4, 0, 60 * MINUTE_MS)
        .await
        .expect("direct L2 to L4 row should read");
    assert_eq!(l4.len(), 1);
    assert_eq!(l4[0].sample_count, 2_400);
    let coverage = store
        .history_coverage(&DashboardSettings::default())
        .await
        .expect("coverage should reflect disabled tier state");
    assert!(!coverage.tiers[2].enabled);
    assert!(coverage.tiers[3].enabled);

    store
        .insert_snapshot(30_000, &snapshot(30_000, 90.0))
        .await
        .expect("late sample should respect the disabled tier state");
    let l3_after_late_write = store
        .read_tier_buckets(Tier::L3, 0, 5 * MINUTE_MS)
        .await
        .expect("dormant L3 row after late write should read");
    assert_eq!(l3_after_late_write.len(), 1);
    assert_eq!(l3_after_late_write[0].sample_count, 999);
}

#[tokio::test]
async fn detail_rows_written_at_detail_interval() {
    // Break caught: filesystem/process detail is written every poll or not
    // written again once the configured cadence elapses.
    let fixture = TempDatabase::new("detail-cadence");
    let store = fixture.store().await;
    for captured_at_ms in [0, 1_500, 61_000] {
        store
            .insert_snapshot(captured_at_ms, &snapshot(captured_at_ms, 10.0))
            .await
            .expect("fixture sample should insert");
    }
    let mut replacement = snapshot(61_000, 20.0);
    replacement.filesystems[0].used_percent = 77.0;
    replacement.processes[0].cpu_percent = 88.0;
    store
        .insert_snapshot(61_000, &replacement)
        .await
        .expect("same-timestamp detail should upsert with its raw sample");
    let report = maintain_with_config(&store, &config(), 62_000)
        .await
        .expect("maintenance should report the latest detail write");
    assert_eq!(report.detail_rows, 2);

    let pool = fixture.pool().await;
    let fs_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fs_samples")
        .fetch_one(&pool)
        .await
        .expect("filesystem detail count");
    let process_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM process_samples")
        .fetch_one(&pool)
        .await
        .expect("process detail count");
    assert_eq!(fs_rows, 2);
    assert_eq!(process_rows, 2);
    let newest_fs_percent: f64 =
        sqlx::query_scalar("SELECT used_percent FROM fs_samples WHERE captured_at_ms = 61000")
            .fetch_one(&pool)
            .await
            .expect("replacement filesystem detail value");
    let newest_process_cpu: f64 =
        sqlx::query_scalar("SELECT cpu_percent FROM process_samples WHERE captured_at_ms = 61000")
            .fetch_one(&pool)
            .await
            .expect("replacement process detail value");
    assert_eq!(newest_fs_percent, 77.0);
    assert_eq!(newest_process_cpu, 88.0);
    pool.close().await;
}

#[tokio::test]
async fn legacy_l2_null_minima_fall_back_to_averages() {
    // Break caught: a pre-v1 rollup whose additive minimum columns are NULL
    // cannot be promoted because the tier reader rejects NULL as f64.
    let fixture = TempDatabase::new("legacy-minima");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    sqlx::query(
        r#"
        INSERT INTO metric_rollups_1m (
          bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
          avg_cpu_usage_percent, max_cpu_usage_percent,
          avg_memory_used_percent, max_memory_used_percent,
          avg_swap_used_percent, max_swap_used_percent,
          avg_load_percent, max_load_percent, avg_root_used_percent
        ) VALUES (0, 1, 59_999, 40, 10, 20, 30, 40, 50, 60, 70, 80, 90)
        "#,
    )
    .execute(&pool)
    .await
    .expect("legacy L2 fixture row should insert");
    pool.close().await;

    let rows = store
        .read_tier_buckets(Tier::L2, 0, MINUTE_MS)
        .await
        .expect("legacy L2 row should read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cpu.min, 10.0);
    assert_eq!(rows[0].memory.min, 30.0);
    assert_eq!(rows[0].swap.min, 50.0);
    assert_eq!(rows[0].load.min, 70.0);
    assert_eq!(rows[0].root_used.map(|stat| stat.min), Some(90.0));
    assert_eq!(rows[0].root_used.map(|stat| stat.max), Some(90.0));
}

#[tokio::test]
async fn history_coverage_reports_every_tier_and_json_horizon() {
    // Break caught: the storage coverage response reports only legacy L1/L2
    // totals or omits the oldest raw row that still has a complete snapshot.
    let fixture = TempDatabase::new("ladder-coverage");
    let store = fixture.store().await;
    store
        .insert_snapshot(1_000, &snapshot(1_000, 10.0))
        .await
        .expect("raw fixture sample should insert");
    store
        .upsert_tier_bucket(Tier::L3, &bucket(0, 5 * MINUTE_MS, 40, 10.0))
        .await
        .expect("L3 fixture bucket should upsert");
    store
        .upsert_tier_bucket(Tier::L4, &bucket(0, 60 * MINUTE_MS, 40, 10.0))
        .await
        .expect("L4 fixture bucket should upsert");

    let coverage = store
        .history_coverage(&DashboardSettings::default())
        .await
        .expect("ladder coverage should read");
    assert_eq!(coverage.snapshot_json_oldest_ms, Some(1_000));
    assert_eq!(
        coverage
            .tiers
            .iter()
            .map(|tier| (tier.tier.as_str(), tier.bucket_count))
            .collect::<Vec<_>>(),
        vec![("l1", 1), ("l2", 1), ("l3", 1), ("l4", 1)]
    );
    assert_eq!(coverage.tiers[0].resolution_ms, 1_500);
    assert_eq!(coverage.tiers[1].resolution_ms, MINUTE_MS);
    assert_eq!(coverage.tiers[2].resolution_ms, 5 * MINUTE_MS);
    assert_eq!(coverage.tiers[3].resolution_ms, 60 * MINUTE_MS);
}

fn snapshot(captured_at_ms: i64, cpu: f64) -> SystemSnapshot {
    SystemSnapshot {
        timestamp: format!("fixture-{captured_at_ms}"),
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
            parent_pid: None,
            started_at: None,
        }],
    }
}
