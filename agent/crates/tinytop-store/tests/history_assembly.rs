use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use tinytop_store::{
    HistoryQuery, SqliteHistoryStore,
    maintenance::{LadderConfig, maintain_with_config},
};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

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
            "tinytop-history-assembly-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("fixture directory");
        Self {
            url: format!("sqlite://{}", dir.join("history.sqlite").display()),
            dir,
        }
    }

    async fn raw_pool(&self) -> SqlitePool {
        SqlitePool::connect_with(
            SqliteConnectOptions::from_str(&self.url)
                .expect("fixture URL")
                .create_if_missing(false),
        )
        .await
        .expect("raw pool")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn filesystem(mount: &str, used_bytes: u64) -> FilesystemSnapshot {
    FilesystemSnapshot {
        filesystem: format!("/dev/{mount}"),
        fs_type: "ext4".to_string(),
        size_bytes: 1_000,
        used_bytes,
        available_bytes: 1_000 - used_bytes,
        used_percent: used_bytes as f64 / 10.0,
        mount: mount.to_string(),
        inode_used_percent: Some(25.0),
        inode_used: Some(25),
        inode_total: Some(100),
    }
}

fn snapshot(stamp: Option<i64>) -> SystemSnapshot {
    SystemSnapshot {
        timestamp: "2026-08-30T00:00:00Z".to_string(),
        filesystems_captured_at_ms: stamp,
        identity: IdentitySnapshot {
            hostname: "host".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Ubuntu".to_string(),
            kernel: "6.8.0".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Linux,
                confidence: RuntimeConfidence::High,
                reason: "fixture".to_string(),
            },
            uptime_seconds: 12_345,
        },
        cpu: CpuSnapshot {
            usage_percent: 12.5,
            cores: 8,
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
            free_bytes: 25,
            used_bytes: 75,
            used_percent: 75.0,
        },
        load: LoadSnapshot {
            one: 1.0,
            five: 2.0,
            fifteen: 3.0,
            runnable: Some(4),
            total_threads: Some(500),
            last_pid: Some(9_999),
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: vec![filesystem("/", 500), filesystem("/data", 250)],
        processes: (0..3)
            .map(|rank| ProcessSnapshot {
                pid: 100 + rank,
                command: format!("process-{rank} --flag"),
                cpu_percent: rank as f64 + 0.5,
                memory_percent: rank as f64 + 1.5,
                rss_bytes: 1_000 + u64::from(rank),
                parent_pid: Some(1),
                started_at: Some(format!("2026-08-30T00:00:0{rank}Z")),
                gpu_percent: None,
            })
            .collect(),
        gpus: Vec::new(),
        sensors: Vec::new(),
    }
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    let sql = match table {
        "fs_samples" => "SELECT COUNT(*) FROM fs_samples",
        "fs_mount_events" => "SELECT COUNT(*) FROM fs_mount_events",
        "host_identity" => "SELECT COUNT(*) FROM host_identity",
        other => panic!("unsupported fixture table {other}"),
    };
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

#[tokio::test]
async fn assembled_snapshot_round_trips_every_stored_field() {
    let fixture = TempDatabase::new("round-trip");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let input = snapshot(Some(10_000));
    let written = store.insert_snapshot(10_500, &input).await.expect("insert");
    let read = store
        .read_history(HistoryQuery::default())
        .await
        .expect("history");

    assert_eq!(read, vec![written.clone()]);
    let output = &written.snapshot;
    assert_eq!(written.captured_at_ms, 10_500);
    assert_eq!(output.timestamp, input.timestamp);
    assert_eq!(output.identity, input.identity);
    assert_eq!(output.cpu.usage_percent, input.cpu.usage_percent);
    assert_eq!(output.cpu.cores, input.cpu.cores);
    assert!(output.cpu.times.is_none());
    assert_eq!(output.memory, input.memory);
    assert_eq!(output.swap, input.swap);
    assert_eq!(output.load, input.load);
    assert_eq!(output.filesystems, input.filesystems);
    assert_eq!(output.processes, input.processes);
    assert_eq!(output.filesystems_captured_at_ms, Some(10_000));
    assert!(output.pressure.cpu.some.is_none() && output.pressure.cpu.full.is_none());
    assert!(output.pressure.memory.some.is_none() && output.pressure.memory.full.is_none());
    assert!(output.pressure.io.some.is_none() && output.pressure.io.full.is_none());
}

#[tokio::test]
async fn filesystem_rows_are_written_only_on_change_and_events_on_presence() {
    let fixture = TempDatabase::new("filesystem-change");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let pool = fixture.raw_pool().await;

    let tick1 = snapshot(Some(1_000));
    store.insert_snapshot(1_500, &tick1).await.expect("tick 1");
    assert_eq!(count(&pool, "fs_samples").await, 2);
    assert_eq!(count(&pool, "fs_mount_events").await, 2);

    store.insert_snapshot(3_000, &tick1).await.expect("tick 2");
    assert_eq!(count(&pool, "fs_samples").await, 2);
    assert_eq!(count(&pool, "fs_mount_events").await, 2);

    let mut tick3 = snapshot(Some(4_000));
    tick3.filesystems[1] = filesystem("/data", 251);
    store.insert_snapshot(4_500, &tick3).await.expect("tick 3");
    assert_eq!(count(&pool, "fs_samples").await, 3);
    assert_eq!(count(&pool, "fs_mount_events").await, 2);

    let mut tick4 = snapshot(Some(5_500));
    tick4.filesystems.pop();
    store.insert_snapshot(6_000, &tick4).await.expect("tick 4");
    assert_eq!(count(&pool, "fs_samples").await, 3);
    assert_eq!(count(&pool, "fs_mount_events").await, 3);

    let tick5 = snapshot(Some(7_000));
    store.insert_snapshot(7_500, &tick5).await.expect("tick 5");
    assert_eq!(count(&pool, "fs_samples").await, 4);
    assert_eq!(count(&pool, "fs_mount_events").await, 4);
    let events: Vec<(i64, String, i64)> = sqlx::query(
        "SELECT captured_at_ms, mount, present FROM fs_mount_events ORDER BY captured_at_ms, mount",
    )
    .fetch_all(&pool)
    .await
    .expect("mount events")
    .into_iter()
    .map(|row| {
        (
            row.get("captured_at_ms"),
            row.get("mount"),
            row.get("present"),
        )
    })
    .collect();
    assert_eq!(
        events,
        [
            (1_000, "/".to_string(), 1),
            (1_000, "/data".to_string(), 1),
            (5_500, "/data".to_string(), 0),
            (7_000, "/data".to_string(), 1),
        ]
    );
    let stamps: Vec<Option<i64>> = sqlx::query(
        "SELECT filesystems_captured_at_ms FROM metric_samples ORDER BY captured_at_ms",
    )
    .fetch_all(&pool)
    .await
    .expect("stamps")
    .into_iter()
    .map(|row| row.get("filesystems_captured_at_ms"))
    .collect();
    assert_eq!(
        stamps,
        [
            Some(1_000),
            Some(1_000),
            Some(4_000),
            Some(5_500),
            Some(7_000)
        ]
    );
}

#[tokio::test]
async fn unstamped_snapshot_keys_filesystems_by_captured_at() {
    let fixture = TempDatabase::new("unstamped");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let pool = fixture.raw_pool().await;
    let mut first = snapshot(None);
    first.filesystems.truncate(1);
    store.insert_snapshot(1_000, &first).await.expect("first");
    store
        .insert_snapshot(2_000, &first)
        .await
        .expect("unchanged");
    assert_eq!(count(&pool, "fs_samples").await, 1);
    let mut changed = first;
    changed.filesystems[0].used_bytes += 1;
    store
        .insert_snapshot(3_000, &changed)
        .await
        .expect("changed");
    assert_eq!(count(&pool, "fs_samples").await, 2);
}

#[tokio::test]
async fn identity_is_interned_once_until_a_string_changes() {
    let fixture = TempDatabase::new("identity");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let pool = fixture.raw_pool().await;
    let input = snapshot(Some(1_000));
    for captured_at_ms in [1_500, 3_000, 4_500] {
        store
            .insert_snapshot(captured_at_ms, &input)
            .await
            .expect("same identity");
    }
    assert_eq!(count(&pool, "host_identity").await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT identity_id) FROM metric_samples")
            .fetch_one(&pool)
            .await
            .expect("identity refs"),
        1
    );
    let mut changed = input;
    changed.identity.kernel = "6.9.0".to_string();
    store
        .insert_snapshot(6_000, &changed)
        .await
        .expect("new identity");
    assert_eq!(count(&pool, "host_identity").await, 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT first_seen_ms FROM host_identity WHERE kernel = '6.9.0'",
        )
        .fetch_one(&pool)
        .await
        .expect("first seen"),
        6_000
    );
}

#[tokio::test]
async fn state_is_primed_at_connect() {
    let fixture = TempDatabase::new("primed");
    let input = snapshot(Some(1_000));
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    store.insert_snapshot(1_500, &input).await.expect("first");
    let pool = fixture.raw_pool().await;
    let pending_before: i64 = sqlx::query_scalar(
        "SELECT CAST(value_json AS INTEGER) FROM history_state WHERE state_key = 'pendingDetailRows'",
    )
    .fetch_one(&pool)
    .await
    .expect("pending detail rows before reopen");
    store.close().await.expect("close");

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("reopen");
    store
        .insert_snapshot(3_000, &input)
        .await
        .expect("same after reopen");
    assert_eq!(count(&pool, "host_identity").await, 1);
    assert_eq!(count(&pool, "fs_samples").await, 2);
    assert_eq!(count(&pool, "fs_mount_events").await, 2);
    let pending_after: i64 = sqlx::query_scalar(
        "SELECT CAST(value_json AS INTEGER) FROM history_state WHERE state_key = 'pendingDetailRows'",
    )
    .fetch_one(&pool)
    .await
    .expect("pending detail rows after reopen");
    assert_eq!(pending_after, pending_before);
}

#[tokio::test]
async fn concurrent_snapshot_writes_preserve_presence_transitions() {
    let fixture = TempDatabase::new("concurrent-presence");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let mut initial = snapshot(Some(1_000));
    initial
        .filesystems
        .retain(|filesystem| filesystem.mount == "/");
    store
        .insert_snapshot(1_500, &initial)
        .await
        .expect("initial root-only snapshot");

    let appears_store = store.clone();
    let appears = tokio::spawn(async move {
        appears_store
            .insert_snapshot(2_500, &snapshot(Some(2_000)))
            .await
    });
    tokio::task::yield_now().await;
    let disappears_store = store.clone();
    let disappears = tokio::spawn(async move {
        let mut without_data = snapshot(Some(3_000));
        without_data
            .filesystems
            .retain(|filesystem| filesystem.mount == "/");
        disappears_store.insert_snapshot(3_500, &without_data).await
    });
    appears
        .await
        .expect("appearance task should join")
        .expect("appearance tick should write");
    disappears
        .await
        .expect("disappearance task should join")
        .expect("disappearance tick should write");

    let pool = fixture.raw_pool().await;
    let events: Vec<(i64, i64)> = sqlx::query(
        "SELECT captured_at_ms, present FROM fs_mount_events WHERE mount = '/data' ORDER BY captured_at_ms",
    )
    .fetch_all(&pool)
    .await
    .expect("data presence events")
    .into_iter()
    .map(|row| (row.get("captured_at_ms"), row.get("present")))
    .collect();
    assert_eq!(events, [(2_000, 1), (3_000, 0)]);
}

#[tokio::test]
async fn regressing_filesystem_key_cannot_reintroduce_a_mount_into_future_history() {
    let fixture = TempDatabase::new("regressing-filesystem-key");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let mut root_only = snapshot(Some(1_000));
    root_only
        .filesystems
        .retain(|filesystem| filesystem.mount == "/");
    store
        .insert_snapshot(1_500, &root_only)
        .await
        .expect("initial root-only snapshot");

    root_only.filesystems_captured_at_ms = Some(3_000);
    store
        .insert_snapshot(3_500, &root_only)
        .await
        .expect("newer root-only snapshot");
    let late = store
        .insert_snapshot(2_500, &snapshot(Some(2_000)))
        .await
        .expect("late snapshot is stored without regressing filesystem state");
    assert!(
        late.snapshot
            .filesystems
            .iter()
            .all(|row| row.mount != "/data")
    );

    root_only.filesystems_captured_at_ms = Some(4_000);
    let future = store
        .insert_snapshot(4_500, &root_only)
        .await
        .expect("future root-only snapshot");
    assert!(
        future
            .snapshot
            .filesystems
            .iter()
            .all(|row| row.mount != "/data")
    );

    let pool = fixture.raw_pool().await;
    let data_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM fs_samples WHERE mount = '/data'")
            .fetch_one(&pool)
            .await
            .expect("data filesystem rows");
    let data_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM fs_mount_events WHERE mount = '/data'")
            .fetch_one(&pool)
            .await
            .expect("data filesystem events");
    assert_eq!((data_rows, data_events), (0, 0));
}

#[tokio::test]
async fn filesystems_carry_forward_until_the_mount_disappears() {
    let fixture = TempDatabase::new("carry-forward");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let at_zero = snapshot(Some(0));
    store.insert_snapshot(1, &at_zero).await.expect("initial");
    let unchanged = snapshot(Some(5 * 60_000));
    store
        .insert_snapshot(7 * 60_000, &unchanged)
        .await
        .expect("carried");
    let mut gone = snapshot(Some(10 * 60_000));
    gone.filesystems
        .retain(|filesystem| filesystem.mount != "/data");
    store
        .insert_snapshot(12 * 60_000, &gone)
        .await
        .expect("gone");
    let history = store
        .read_history(HistoryQuery::default())
        .await
        .expect("history");
    let carried_data = history[1]
        .snapshot
        .filesystems
        .iter()
        .find(|row| row.mount == "/data")
        .expect("the unchanged /data mount should carry forward");
    let at_zero_data = at_zero
        .filesystems
        .iter()
        .find(|row| row.mount == "/data")
        .expect("the initial snapshot should contain /data");
    assert_eq!(carried_data, at_zero_data);

    let carried_root = history[1]
        .snapshot
        .filesystems
        .iter()
        .find(|row| row.mount == "/")
        .expect("the unchanged root mount should be assembled");
    let unchanged_root = unchanged
        .filesystems
        .iter()
        .find(|row| row.mount == "/")
        .expect("the unchanged snapshot should contain the root mount");
    assert_eq!(carried_root, unchanged_root);
    assert!(
        !history[2]
            .snapshot
            .filesystems
            .iter()
            .any(|row| row.mount == "/data")
    );
}

#[tokio::test]
async fn rows_without_identity_are_not_read() {
    let fixture = TempDatabase::new("no-identity");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let pool = fixture.raw_pool().await;
    sqlx::query(
        r#"
        INSERT INTO metric_samples (
          captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
          cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
          memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
          load_one, load_five, load_fifteen, load_percent,
          runnable_threads, total_threads, root_used_percent, identity_id
        ) VALUES (1, 'raw', 'raw', 'Linux', 1, 1, 1, 1, 1, 1, 1, 1,
                  1, 1, 1, 1, NULL, NULL, NULL, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("raw row");
    assert!(
        store
            .read_history(HistoryQuery::default())
            .await
            .expect("history")
            .is_empty()
    );
}

#[tokio::test]
async fn processes_fall_back_to_the_minute_tier_within_two_intervals() {
    let fixture = TempDatabase::new("process-fallback");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let input = snapshot(Some(1_000));
    store.insert_snapshot(1_000, &input).await.expect("seed");
    let pool = fixture.raw_pool().await;

    for captured_at_ms in [119_000_i64, 122_001] {
        sqlx::query(
            r#"
            INSERT INTO metric_samples (
              captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
              cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
              memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
              load_one, load_five, load_fifteen, load_percent, runnable_threads,
              total_threads, root_used_percent, identity_id, uptime_seconds,
              memory_available_bytes, swap_free_bytes, last_pid,
              filesystems_captured_at_ms
            )
            SELECT ?, 'fallback', hostname, runtime_kind,
                   cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
                   memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
                   load_one, load_five, load_fifteen, load_percent, runnable_threads,
                   total_threads, root_used_percent, identity_id, uptime_seconds,
                   memory_available_bytes, swap_free_bytes, last_pid,
                   filesystems_captured_at_ms
            FROM metric_samples WHERE captured_at_ms = 1000
            "#,
        )
        .bind(captured_at_ms)
        .execute(&pool)
        .await
        .expect("synthetic metric row");
    }
    let history = store
        .read_history(HistoryQuery {
            since_ms: Some(119_000),
            until_ms: Some(122_001),
            limit: Some(2),
        })
        .await
        .expect("history");
    assert_eq!(history[0].snapshot.processes, input.processes);
    assert!(history[1].snapshot.processes.is_empty());
}

#[tokio::test]
async fn prune_keeps_the_carry_forward_floor() {
    let fixture = TempDatabase::new("prune-floor");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let first = snapshot(Some(1_000));
    store.insert_snapshot(1_500, &first).await.expect("first");
    let mut second = snapshot(Some(2_000));
    second
        .filesystems
        .retain(|filesystem| filesystem.mount == "/");
    store.insert_snapshot(2_500, &second).await.expect("second");

    maintain_with_config(
        &store,
        &LadderConfig {
            l1_keep_ms: i64::MAX,
            l2_keep_ms: 10_000,
            l3: None,
            l4: None,
            detail_interval_ms: 60_000,
            process_fast_keep_ms: i64::MAX,
            poll_interval_ms: 1_500,
        },
        100_000,
    )
    .await
    .expect("maintenance");
    let pool = fixture.raw_pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fs_samples WHERE mount = '/'")
            .fetch_one(&pool)
            .await
            .expect("root rows"),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fs_mount_events WHERE mount = '/'")
            .fetch_one(&pool)
            .await
            .expect("root events"),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fs_samples WHERE mount = '/data'")
            .fetch_one(&pool)
            .await
            .expect("data rows"),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fs_mount_events WHERE mount = '/data'",)
            .fetch_one(&pool)
            .await
            .expect("data events"),
        0
    );
}

#[tokio::test]
async fn a_process_row_failure_does_not_fail_the_tick() {
    let fixture = TempDatabase::new("process-failure");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let pool = fixture.raw_pool().await;
    sqlx::query("DROP TABLE process_commands")
        .execute(&pool)
        .await
        .expect("process command table should drop");

    let written = store
        .insert_snapshot(1_500, &snapshot(Some(1_000)))
        .await
        .expect("metric tick must commit despite process failure");
    assert!(written.snapshot.processes.is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM metric_samples")
            .fetch_one(&pool)
            .await
            .expect("metric count"),
        1
    );
    assert!(
        store.read_history(HistoryQuery::default()).await.is_err(),
        "normal history reads must surface process-schema damage"
    );
}

#[tokio::test]
async fn history_window_of_2400_rows_assembles_under_budget() {
    let fixture = TempDatabase::new("performance");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let mut seed = snapshot(Some(0));
    seed.processes = (0..8)
        .map(|rank| ProcessSnapshot {
            pid: 1_000 + rank,
            command: format!("perf-{rank}"),
            cpu_percent: rank as f64,
            memory_percent: rank as f64,
            rss_bytes: 10_000 + u64::from(rank),
            parent_pid: Some(1),
            started_at: Some(format!("2026-08-30T00:00:0{rank}Z")),
            gpu_percent: None,
        })
        .collect();
    store.insert_snapshot(0, &seed).await.expect("seed");
    let pool = fixture.raw_pool().await;
    let mut transaction = pool.begin().await.expect("bulk transaction");
    sqlx::raw_sql(
        r#"
        WITH RECURSIVE ticks(i) AS (
          VALUES(1) UNION ALL SELECT i + 1 FROM ticks WHERE i < 2399
        )
        INSERT INTO metric_samples (
          captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
          cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
          memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
          load_one, load_five, load_fifteen, load_percent, runnable_threads,
          total_threads, root_used_percent, identity_id, uptime_seconds,
          memory_available_bytes, swap_free_bytes, last_pid,
          filesystems_captured_at_ms
        )
        SELECT i * 1500, printf('perf-%d', i), m.hostname, m.runtime_kind,
               m.cpu_usage_percent, m.cpu_cores, m.memory_used_percent, m.memory_used_bytes,
               m.memory_total_bytes, m.swap_used_percent, m.swap_used_bytes, m.swap_total_bytes,
               m.load_one, m.load_five, m.load_fifteen, m.load_percent, m.runnable_threads,
               m.total_threads, m.root_used_percent, m.identity_id, m.uptime_seconds,
               m.memory_available_bytes, m.swap_free_bytes, m.last_pid,
               (i / 40) * 60000
        FROM ticks CROSS JOIN metric_samples m
        WHERE m.captured_at_ms = 0;

        WITH RECURSIVE ticks(i) AS (
          VALUES(1) UNION ALL SELECT i + 1 FROM ticks WHERE i < 2399
        ), ranks(rank) AS (
          VALUES(0),(1),(2),(3),(4),(5),(6),(7)
        )
        INSERT INTO process_samples_fast (
          captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
          rss_bytes, parent_pid, started_at_ms, gpu_percent
        )
        SELECT ticks.i * 1500, ranks.rank, 1000 + ranks.rank, c.command_id,
               ranks.rank, ranks.rank, 10000 + ranks.rank, 1,
               1788048000000 + ranks.rank * 1000, NULL
        FROM ticks CROSS JOIN ranks
        JOIN process_commands c ON c.command = printf('perf-%d', ranks.rank);
        "#,
    )
    .execute(&mut *transaction)
    .await
    .expect("synthetic window");
    transaction.commit().await.expect("bulk commit");

    let started = Instant::now();
    let history = store
        .read_history(HistoryQuery {
            since_ms: Some(0),
            until_ms: Some(3_598_500),
            limit: Some(2_400),
        })
        .await
        .expect("history window");
    let elapsed_ms = started.elapsed().as_millis();
    println!("history_window_of_2400_rows_assembles_under_budget: {elapsed_ms} ms");
    assert_eq!(history.len(), 2_400);
    assert!(
        history
            .iter()
            .all(|sample| sample.snapshot.processes.len() == 8),
        "every synthetic tick must carry eight fast process rows"
    );
    assert!(elapsed_ms < 2_000, "debug assembly took {elapsed_ms} ms");
}

#[tokio::test]
async fn history_window_of_2400_unstamped_rows_uses_batched_filesystem_replay() {
    let fixture = TempDatabase::new("performance-unstamped");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("store");
    let mut seed = snapshot(None);
    seed.processes.clear();
    for index in 0_i64..2_400 {
        let captured_at_ms = index.saturating_mul(1_500).saturating_add(1);
        seed.timestamp = format!("2026-08-30T00:{:02}:{:02}Z", index / 60, index % 60);
        store
            .insert_snapshot(captured_at_ms, &seed)
            .await
            .expect("synthetic unstamped tick");
    }

    let started = Instant::now();
    let history = store
        .read_history(HistoryQuery {
            limit: Some(2_400),
            ..HistoryQuery::default()
        })
        .await
        .expect("unstamped history should assemble");
    let elapsed_ms = started.elapsed().as_millis();
    println!(
        "history_window_of_2400_unstamped_rows_uses_batched_filesystem_replay: {elapsed_ms} ms"
    );
    assert_eq!(history.len(), 2_400);
    assert!(
        elapsed_ms < 2_000,
        "debug unstamped assembly took {elapsed_ms} ms"
    );
}
