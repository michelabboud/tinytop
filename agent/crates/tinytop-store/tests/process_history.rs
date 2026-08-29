use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tinytop_store::{
    DashboardSettings, HistoryQuery, ProcessHistorySource, SqliteHistoryStore,
    maintenance::maintain_with_config,
};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

const HOUR_MS: i64 = 3_600_000;

struct TempDatabase {
    dir: PathBuf,
    url: String,
}

impl TempDatabase {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should follow the Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-process-history-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("fixture directory should be created");
        let url = format!("sqlite://{}", dir.join("history.sqlite").display());
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

#[tokio::test]
async fn fast_rows_every_tick_and_minute_rows_every_interval() {
    let fixture = TempDatabase::new("cadence");
    let store = fixture.store().await;
    let t = current_time_ms();
    let first_snapshot = snapshot(t);
    let fixture_command = first_snapshot.processes[0].command.clone();
    store
        .insert_snapshot(t, &first_snapshot)
        .await
        .expect("snapshot should insert");
    for captured_at_ms in [t + 1_500, t + 3_000] {
        store
            .insert_snapshot(captured_at_ms, &snapshot(captured_at_ms))
            .await
            .expect("snapshot should insert");
    }

    let pool = fixture.pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM process_samples_fast")
            .fetch_one(&pool)
            .await
            .expect("fast process count should read"),
        12
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT captured_at_ms) FROM process_samples")
            .fetch_one(&pool)
            .await
            .expect("minute capture count should read"),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM process_commands")
            .fetch_one(&pool)
            .await
            .expect("command count should read"),
        2
    );
    let fast_command_id: i64 = sqlx::query_scalar(
        "SELECT command_id FROM process_samples_fast WHERE captured_at_ms = ? AND rank = ?",
    )
    .bind(t)
    .bind(0_i64)
    .fetch_one(&pool)
    .await
    .expect("fast command identifier should read");
    let minute_command_id: i64 = sqlx::query_scalar(
        "SELECT command_id FROM process_samples WHERE captured_at_ms = ? AND rank = ?",
    )
    .bind(t)
    .bind(0_i64)
    .fetch_one(&pool)
    .await
    .expect("minute command identifier should read");
    let dictionary_command_id: i64 =
        sqlx::query_scalar("SELECT command_id FROM process_commands WHERE command = ?")
            .bind(&fixture_command)
            .fetch_one(&pool)
            .await
            .expect("dictionary command identifier should read");
    assert_eq!(fast_command_id, minute_command_id);
    assert_eq!(fast_command_id, dictionary_command_id);
    pool.close().await;
}

#[tokio::test]
async fn read_history_processes_picks_fast_inside_the_keep_window_and_minute_outside() {
    let fixture = TempDatabase::new("source-selection");
    let store = fixture.store().await;
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.process_fast_keep_hours = 1;
    store
        .put_settings(&settings)
        .await
        .expect("settings should save");
    let now_ms = current_time_ms();
    let captured_at_ms = now_ms - 10 * 60_000;
    let history_snapshot = snapshot(captured_at_ms);
    let fixture_command = history_snapshot.processes[0].command.clone();
    store
        .insert_snapshot(captured_at_ms, &history_snapshot)
        .await
        .expect("snapshot should insert");
    let fast_only_captured_at_ms = now_ms - 5 * 60_000;
    let pool = fixture.pool().await;
    let command_id: i64 =
        sqlx::query_scalar("SELECT command_id FROM process_commands WHERE command = ?")
            .bind(&fixture_command)
            .fetch_one(&pool)
            .await
            .expect("fixture command identifier should read");
    sqlx::query(
        "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at, gpu_percent) VALUES (?, 0, 424242, ?, 1.0, 2.0, 3, NULL, NULL, NULL)",
    )
    .bind(fast_only_captured_at_ms)
    .bind(command_id)
    .execute(&pool)
    .await
    .expect("fast-only process fixture should insert");
    pool.close().await;

    let fast = store
        .read_history_processes(HistoryQuery {
            since_ms: Some(now_ms - 30 * 60_000),
            until_ms: Some(now_ms),
            limit: Some(10),
        })
        .await
        .expect("fast history should read");
    assert_eq!(fast.source, ProcessHistorySource::Fast);
    assert_eq!(fast.captures.len(), 2);
    assert!(
        fast.captures
            .iter()
            .flat_map(|capture| &capture.processes)
            .any(|process| process.pid == 424_242)
    );

    let minute = store
        .read_history_processes(HistoryQuery {
            since_ms: Some(now_ms - 2 * HOUR_MS),
            until_ms: Some(now_ms),
            limit: Some(10),
        })
        .await
        .expect("minute history should read");
    assert_eq!(minute.source, ProcessHistorySource::Minute);
    assert_eq!(minute.captures.len(), 1);
    assert!(
        minute
            .captures
            .iter()
            .flat_map(|capture| &capture.processes)
            .all(|process| process.pid != 424_242)
    );

    let open = store
        .read_history_processes(HistoryQuery {
            since_ms: None,
            until_ms: Some(now_ms),
            limit: Some(10),
        })
        .await
        .expect("open-ended history should read");
    assert_eq!(open.source, ProcessHistorySource::Minute);
    assert_eq!(open.captures.len(), 1);
    assert!(
        open.captures
            .iter()
            .flat_map(|capture| &capture.processes)
            .all(|process| process.pid != 424_242)
    );

    let expected_commands: Vec<&str> = history_snapshot
        .processes
        .iter()
        .map(|process| process.command.as_str())
        .collect();
    for capture in fast
        .captures
        .iter()
        .chain(&minute.captures)
        .chain(&open.captures)
    {
        if capture.captured_at_ms == fast_only_captured_at_ms {
            assert_eq!(capture.processes.len(), 1);
            assert_eq!(capture.processes[0].pid, 424_242);
            assert_eq!(
                capture.processes[0].command.as_str(),
                fixture_command.as_str()
            );
        } else {
            assert_eq!(capture.captured_at_ms, captured_at_ms);
            let commands: Vec<&str> = capture
                .processes
                .iter()
                .map(|process| process.command.as_str())
                .collect();
            assert_eq!(commands, expected_commands);
        }
    }
}

#[tokio::test]
async fn prune_process_fast_history_is_limit_bounded_and_leaves_no_orphans() {
    let fixture = TempDatabase::new("prune");
    let store = fixture.store().await;
    let now_ms = current_time_ms();
    let old_ms = now_ms - 2 * HOUR_MS;
    let new_ms = now_ms - 30 * 60_000;
    let pool = fixture.pool().await;
    for command in ["old-only", "new-only", "minute-only", "never-referenced"] {
        sqlx::query("INSERT INTO process_commands (command) VALUES (?)")
            .bind(command)
            .execute(&pool)
            .await
            .expect("command fixture should insert");
    }
    let old_id: i64 =
        sqlx::query_scalar("SELECT command_id FROM process_commands WHERE command = 'old-only'")
            .fetch_one(&pool)
            .await
            .expect("old command id should read");
    let new_id: i64 =
        sqlx::query_scalar("SELECT command_id FROM process_commands WHERE command = 'new-only'")
            .fetch_one(&pool)
            .await
            .expect("new command id should read");
    let minute_id: i64 =
        sqlx::query_scalar("SELECT command_id FROM process_commands WHERE command = 'minute-only'")
            .fetch_one(&pool)
            .await
            .expect("minute command id should read");
    sqlx::query(
        "WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM seq WHERE n < 12000) INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at, gpu_percent) SELECT ?, n, n, ?, 1.0, 2.0, 3, NULL, NULL, NULL FROM seq",
    )
    .bind(old_ms)
    .bind(old_id)
    .execute(&pool)
    .await
    .expect("old fast process fixtures should insert");
    for rank in 1_i64..=10 {
        sqlx::query(
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at, gpu_percent) VALUES (?, ?, ?, ?, 1.0, 2.0, 3, NULL, NULL, NULL)",
        )
        .bind(new_ms)
        .bind(rank)
        .bind(rank)
        .bind(new_id)
        .execute(&pool)
        .await
        .expect("new fast process fixture should insert");
    }
    sqlx::query(
        "INSERT INTO process_samples (captured_at_ms, rank, pid, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at, command_id) VALUES (?, 1, 1, 1.0, 2.0, 3, NULL, NULL, ?)",
    )
    .bind(new_ms)
    .bind(minute_id)
    .execute(&pool)
    .await
    .expect("minute process fixture should insert");
    pool.close().await;

    let mut settings = DashboardSettings::default();
    settings.retention_ladder.process_fast_keep_hours = 1;
    settings.retention_ladder.l1.keep_days = 365;
    settings.retention_ladder.l2.keep_days = 365;
    let config = settings
        .retention_ladder
        .to_ladder_config(settings.poll_interval_ms);
    let report = maintain_with_config(&store, &config, now_ms)
        .await
        .expect("maintenance should prune fast processes");
    assert_eq!(report.process_fast_rows, 12_000);
    assert_eq!(report.orphan_commands, 2);

    let pool = fixture.pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM process_samples_fast")
            .fetch_one(&pool)
            .await
            .expect("surviving fast count should read"),
        10
    );
    let commands: Vec<String> =
        sqlx::query_scalar("SELECT command FROM process_commands ORDER BY command")
            .fetch_all(&pool)
            .await
            .expect("surviving commands should read");
    assert_eq!(commands, ["minute-only", "new-only"]);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .expect("foreign key check should run")
            .is_empty()
    );
    pool.close().await;
}

fn snapshot(captured_at_ms: i64) -> SystemSnapshot {
    SystemSnapshot {
        timestamp: format!("fixture-{captured_at_ms}"),
        filesystems_captured_at_ms: None,
        identity: IdentitySnapshot {
            hostname: "devbox".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Ubuntu".to_string(),
            kernel: "6.8".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Linux,
                confidence: RuntimeConfidence::High,
                reason: "fixture".to_string(),
            },
            uptime_seconds: 60,
        },
        cpu: CpuSnapshot {
            usage_percent: 10.0,
            cores: 4,
            times: Some(CpuTimes::default()),
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
        processes: (0..4)
            .map(|index| ProcessSnapshot {
                pid: 40 + index,
                command: if index % 2 == 0 {
                    "shared-command".to_string()
                } else {
                    "other-command".to_string()
                },
                cpu_percent: index as f64,
                memory_percent: 2.0,
                rss_bytes: 3,
                parent_pid: None,
                started_at: None,
            })
            .collect(),
    }
}

fn current_time_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should follow the Unix epoch")
            .as_millis(),
    )
    .expect("current time should fit in i64")
}
