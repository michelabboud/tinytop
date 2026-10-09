use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tinytop_store::{
    DEFAULT_TOP_PROCESS_COUNT, DashboardSettings, HistoryQuery, MAX_TOP_PROCESS_COUNT,
    ProcessHistorySource, SqliteHistoryStore, maintenance::maintain_with_config,
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
        "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, 0, 424242, ?, 1.0, 2.0, 3, NULL, NULL, NULL)",
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
async fn every_collected_process_is_written_to_both_tables_whatever_the_count() {
    // Break caught: a rank bound somewhere in the write or read path drops the
    // processes beyond the eight that were the previous default.
    for process_count in [
        usize::try_from(DEFAULT_TOP_PROCESS_COUNT).expect("default fits usize"),
        usize::try_from(MAX_TOP_PROCESS_COUNT).expect("maximum fits usize"),
    ] {
        let fixture = TempDatabase::new(&format!("count-{process_count}"));
        let store = fixture.store().await;
        let t = current_time_ms();
        let mut collected = snapshot(t);
        collected.processes = (0..process_count)
            .map(|index| ProcessSnapshot {
                pid: 1_000 + u32::try_from(index).expect("fixture index fits u32"),
                command: format!("fixture-process-{index}"),
                cpu_percent: 0.0,
                memory_percent: 1.0,
                rss_bytes: 4_096,
                parent_pid: None,
                started_at: None,
                gpu_percent: None,
                swap_bytes: None,
                cpu_rank: None,
                memory_rank: None,
            })
            .collect();
        store
            .insert_snapshot(t, &collected)
            .await
            .expect("snapshot should insert");

        let pool = fixture.pool().await;
        let expected_ranks: Vec<i64> =
            (0..i64::try_from(process_count).expect("count fits i64")).collect();
        for (table, sql) in [
            (
                "process_samples_fast",
                "SELECT rank FROM process_samples_fast WHERE captured_at_ms = ? ORDER BY rank",
            ),
            (
                "process_samples",
                "SELECT rank FROM process_samples WHERE captured_at_ms = ? ORDER BY rank",
            ),
        ] {
            let ranks: Vec<i64> = sqlx::query_scalar(sql)
                .bind(t)
                .fetch_all(&pool)
                .await
                .expect("ranks should read");
            assert_eq!(
                ranks, expected_ranks,
                "{table} at {process_count} processes"
            );
        }
        pool.close().await;

        let read = store
            .read_history_processes(HistoryQuery {
                since_ms: Some(t),
                until_ms: Some(t),
                limit: Some(10),
            })
            .await
            .expect("process history should read");
        assert_eq!(read.captures.len(), 1);
        let commands: Vec<&str> = read.captures[0]
            .processes
            .iter()
            .map(|process| process.command.as_str())
            .collect();
        let expected_commands: Vec<String> = (0..process_count)
            .map(|index| format!("fixture-process-{index}"))
            .collect();
        assert_eq!(commands, expected_commands);
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
        "WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM seq WHERE n < 12000) INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) SELECT ?, n, n, ?, 1.0, 2.0, 3, NULL, NULL, NULL FROM seq",
    )
    .bind(old_ms)
    .bind(old_id)
    .execute(&pool)
    .await
    .expect("old fast process fixtures should insert");
    for rank in 1_i64..=10 {
        sqlx::query(
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, ?, 1.0, 2.0, 3, NULL, NULL, NULL)",
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
        "INSERT INTO process_samples (captured_at_ms, rank, pid, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, command_id, gpu_percent) VALUES (?, 1, 1, 1.0, 2.0, 3, NULL, NULL, ?, NULL)",
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

/// A process row with only the fields the two-list tests vary.
fn ranked(
    pid: u32,
    rss_bytes: u64,
    swap_bytes: Option<u64>,
    cpu_rank: Option<u32>,
    memory_rank: Option<u32>,
) -> ProcessSnapshot {
    ProcessSnapshot {
        pid,
        command: format!("ranked-{pid}"),
        cpu_percent: f64::from(pid) / 7.0,
        memory_percent: 1.5,
        rss_bytes,
        parent_pid: Some(1),
        started_at: None,
        gpu_percent: None,
        swap_bytes,
        cpu_rank,
        memory_rank,
    }
}

type StoredRanks = (i64, i64, Option<i64>, Option<i64>, Option<i64>);

/// `(rank, pid, swap_bytes, cpu_rank, memory_rank)` of one capture, straight
/// from the table, in key order.
async fn stored_ranks(pool: &SqlitePool, table: &str, captured_at_ms: i64) -> Vec<StoredRanks> {
    let sql = match table {
        "process_samples_fast" => {
            "SELECT rank, pid, swap_bytes, cpu_rank, memory_rank FROM process_samples_fast WHERE captured_at_ms = ? ORDER BY rank"
        }
        "process_samples" => {
            "SELECT rank, pid, swap_bytes, cpu_rank, memory_rank FROM process_samples WHERE captured_at_ms = ? ORDER BY rank"
        }
        other => panic!("unsupported table {other}"),
    };
    sqlx::query_as(sql)
        .bind(captured_at_ms)
        .fetch_all(pool)
        .await
        .expect("stored ranks should read")
}

fn expected_ranks(processes: &[ProcessSnapshot]) -> Vec<StoredRanks> {
    processes
        .iter()
        .enumerate()
        .map(|(ordinal, process)| {
            (
                i64::try_from(ordinal).expect("ordinal fits i64"),
                i64::from(process.pid),
                process
                    .swap_bytes
                    .map(|bytes| i64::try_from(bytes).expect("fixture swap fits i64")),
                process.cpu_rank.map(i64::from),
                process.memory_rank.map(i64::from),
            )
        })
        .collect()
}

#[tokio::test]
async fn both_lists_are_stored_and_read_back_from_both_tables() {
    // Break caught: a rank is written into the other rank's column, swap or a
    // NULL rank is stored as zero, the memory-only rows are dropped, or `rank`
    // stops being the row ordinal the history routes order by.
    const GIB: u64 = 1 << 30;
    let cases: [(&str, Vec<ProcessSnapshot>); 3] = [
        (
            // N = 3, four rows: in both lists, CPU-only, swap unknown, and a
            // swapped-out process that only the memory list sees.
            "union",
            vec![
                ranked(10, 2 * GIB, Some(0), Some(0), Some(1)),
                ranked(11, 4_096, Some(8_192), Some(1), None),
                ranked(12, 5 * GIB, None, Some(2), Some(0)),
                ranked(13, GIB / 10, Some(2_600_000_000), None, Some(2)),
            ],
        ),
        (
            // N = 2 and the two lists are the same processes: N rows.
            "identical-lists",
            vec![
                ranked(20, GIB, Some(1), Some(0), Some(1)),
                ranked(21, 2 * GIB, Some(2), Some(1), Some(0)),
            ],
        ),
        (
            // N = 2 and the two lists share nothing: 2N rows.
            "disjoint-lists",
            vec![
                ranked(30, 1, Some(0), Some(0), None),
                ranked(31, 2, Some(0), Some(1), None),
                ranked(32, 9 * GIB, Some(GIB), None, Some(0)),
                ranked(33, 8 * GIB, None, None, Some(1)),
            ],
        ),
    ];
    for (label, processes) in cases {
        let fixture = TempDatabase::new(&format!("two-lists-{label}"));
        let store = fixture.store().await;
        let t = current_time_ms();
        let mut collected = snapshot(t);
        collected.processes = processes.clone();

        let stored = store
            .insert_snapshot(t, &collected)
            .await
            .expect("snapshot should insert");
        assert_eq!(
            stored.snapshot.processes, processes,
            "{label}: the sample read back from the store is the sample written"
        );

        let pool = fixture.pool().await;
        for table in ["process_samples_fast", "process_samples"] {
            assert_eq!(
                stored_ranks(&pool, table, t).await,
                expected_ranks(&processes),
                "{label}: {table}"
            );
        }
        pool.close().await;

        let assembled = store
            .read_history(HistoryQuery {
                since_ms: Some(t),
                until_ms: Some(t),
                limit: Some(1),
            })
            .await
            .expect("history should read");
        assert_eq!(assembled.len(), 1, "{label}");
        assert_eq!(assembled[0].snapshot.processes, processes, "{label}");

        for (expected_source, since_ms) in [
            (ProcessHistorySource::Fast, Some(t)),
            (ProcessHistorySource::Minute, None),
        ] {
            let read = store
                .read_history_processes(HistoryQuery {
                    since_ms,
                    until_ms: Some(t),
                    limit: Some(10),
                })
                .await
                .expect("process history should read");
            assert_eq!(read.source, expected_source, "{label}");
            assert_eq!(read.captures.len(), 1, "{label} {expected_source:?}");
            let rows: Vec<StoredRanks> = read.captures[0]
                .processes
                .iter()
                .map(|process| {
                    (
                        process.rank,
                        process.pid,
                        process.swap_bytes,
                        process.cpu_rank,
                        process.memory_rank,
                    )
                })
                .collect();
            assert_eq!(
                rows,
                expected_ranks(&processes),
                "{label} {expected_source:?}"
            );
        }
    }
}

#[tokio::test]
async fn history_process_json_omits_an_unknown_swap_and_an_absent_rank() {
    // Break caught: an absent rank or unknown swap goes on the wire as `null`
    // or `0`, or under a name the live snapshot does not use.
    let fixture = TempDatabase::new("ranks-json");
    let store = fixture.store().await;
    let t = current_time_ms();
    let mut collected = snapshot(t);
    collected.processes = vec![
        ranked(10, 100, Some(7), Some(0), Some(1)),
        ranked(11, 200, None, None, Some(0)),
    ];
    store
        .insert_snapshot(t, &collected)
        .await
        .expect("snapshot should insert");

    let read = store
        .read_history_processes(HistoryQuery {
            since_ms: Some(t),
            until_ms: Some(t),
            limit: Some(1),
        })
        .await
        .expect("process history should read");
    let json = serde_json::to_value(&read.captures[0].processes).expect("rows should serialize");
    assert_eq!(json[0]["rank"], 0);
    assert_eq!(json[0]["swapBytes"], 7);
    assert_eq!(json[0]["cpuRank"], 0);
    assert_eq!(json[0]["memoryRank"], 1);
    assert_eq!(json[1]["rank"], 1);
    assert_eq!(json[1]["memoryRank"], 0);
    let memory_only = json[1].as_object().expect("a process row is an object");
    assert!(!memory_only.contains_key("swapBytes"), "{memory_only:?}");
    assert!(!memory_only.contains_key("cpuRank"), "{memory_only:?}");
}

#[tokio::test]
async fn the_minute_row_is_a_copy_of_the_tick_that_was_due_not_a_fold_of_the_minute() {
    // Break caught: the minute tier averages or merges ranks across the ticks
    // of a minute, or keeps a process from a tick that was not the due one.
    let fixture = TempDatabase::new("minute-copy");
    let store = fixture.store().await;
    let t = current_time_ms() - 120_000;
    let ticks: [(i64, Vec<ProcessSnapshot>); 3] = [
        (
            t,
            vec![
                ranked(10, 100, Some(1), Some(0), Some(1)),
                ranked(11, 900, Some(2), Some(1), Some(0)),
            ],
        ),
        (
            // Inside the same detail interval: pid 10 and 11 trade places and
            // a third process enters. None of this reaches the minute tier.
            t + 1_500,
            vec![
                ranked(11, 950, Some(3), Some(0), Some(1)),
                ranked(10, 100, Some(1), Some(1), None),
                ranked(12, 5_000, Some(4_000), None, Some(0)),
            ],
        ),
        (
            t + 61_000,
            vec![
                ranked(12, 6_000, Some(5_000), Some(0), Some(0)),
                ranked(10, 100, None, Some(1), Some(1)),
            ],
        ),
    ];
    for (captured_at_ms, processes) in &ticks {
        let mut collected = snapshot(*captured_at_ms);
        collected.processes = processes.clone();
        store
            .insert_snapshot(*captured_at_ms, &collected)
            .await
            .expect("snapshot should insert");
    }

    let pool = fixture.pool().await;
    let minute_captures: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT captured_at_ms FROM process_samples ORDER BY captured_at_ms",
    )
    .fetch_all(&pool)
    .await
    .expect("minute captures should read");
    assert_eq!(minute_captures, [t, t + 61_000]);
    for (captured_at_ms, processes) in &ticks {
        assert_eq!(
            stored_ranks(&pool, "process_samples_fast", *captured_at_ms).await,
            expected_ranks(processes),
            "every tick is in the fast tier"
        );
    }
    for index in [0, 2] {
        let (captured_at_ms, processes) = &ticks[index];
        assert_eq!(
            stored_ranks(&pool, "process_samples", *captured_at_ms).await,
            expected_ranks(processes),
            "the minute row equals the fast row of the same tick"
        );
    }
    pool.close().await;
}

#[tokio::test]
async fn rewriting_a_timestamp_replaces_its_rows_and_their_ranks_in_both_tables() {
    // Break caught: a replay with fewer processes leaves the old tail rows, or
    // keeps the first write's ranks beside the second write's values.
    let fixture = TempDatabase::new("replace-ranks");
    let store = fixture.store().await;
    let t = current_time_ms();
    let first = vec![
        ranked(10, 100, Some(1), Some(0), None),
        ranked(11, 200, Some(2), Some(1), Some(1)),
        ranked(12, 300, Some(3), None, Some(0)),
    ];
    let second = vec![
        ranked(12, 300, None, Some(0), Some(1)),
        ranked(10, 400, Some(9), Some(1), Some(0)),
    ];
    for processes in [&first, &second] {
        let mut collected = snapshot(t);
        collected.processes = processes.clone();
        store
            .insert_snapshot(t, &collected)
            .await
            .expect("snapshot should insert");
    }

    let pool = fixture.pool().await;
    for table in ["process_samples_fast", "process_samples"] {
        assert_eq!(
            stored_ranks(&pool, table, t).await,
            expected_ranks(&second),
            "{table}"
        );
    }
    pool.close().await;
}

#[tokio::test]
async fn swap_above_i64_max_is_refused_exactly_as_rss_is_and_the_largest_value_that_fits_is_kept() {
    // Break caught: a u64 that SQLite INTEGER cannot hold is wrapped negative
    // or clamped into a plausible number instead of being refused.
    let too_large = u64::try_from(i64::MAX).expect("i64::MAX fits u64") + 1;
    for (label, overflowing) in [
        ("rss", ranked(11, too_large, Some(1), Some(1), Some(0))),
        ("swap", ranked(11, 1, Some(too_large), Some(1), Some(0))),
    ] {
        let fixture = TempDatabase::new(&format!("overflow-{label}"));
        let store = fixture.store().await;
        let t = current_time_ms();
        let mut collected = snapshot(t);
        collected.processes = vec![ranked(10, 1, Some(1), Some(0), Some(1)), overflowing];

        // The metric row is kept; the tick's process rows are refused whole,
        // in both tiers — no row of the sample is written beside a missing one.
        let stored = store
            .insert_snapshot(t, &collected)
            .await
            .expect("the metric sample is still stored");
        assert_eq!(stored.snapshot.processes, [], "{label}");
        let pool = fixture.pool().await;
        for table in ["process_samples_fast", "process_samples"] {
            assert_eq!(stored_ranks(&pool, table, t).await, [], "{label}: {table}");
        }
        pool.close().await;
    }

    let fixture = TempDatabase::new("overflow-edge");
    let store = fixture.store().await;
    let t = current_time_ms();
    let largest = u64::try_from(i64::MAX).expect("i64::MAX fits u64");
    let mut collected = snapshot(t);
    collected.processes = vec![ranked(10, largest, Some(largest), Some(u32::MAX), Some(0))];
    let stored = store
        .insert_snapshot(t, &collected)
        .await
        .expect("the largest values that fit are stored");
    assert_eq!(stored.snapshot.processes, collected.processes);
}

#[tokio::test]
async fn a_stored_rank_or_swap_outside_its_type_is_refused_on_read() {
    // Break caught: a corrupt negative value is wrapped into a huge u32/u64
    // and served as a real rank or a real swap size.
    for (label, update, expected) in [
        (
            "cpu-rank",
            "UPDATE process_samples_fast SET cpu_rank = -1 WHERE rank = 0",
            "process cpu_rank is outside u32",
        ),
        (
            "memory-rank",
            "UPDATE process_samples_fast SET memory_rank = 4294967296 WHERE rank = 0",
            "process memory_rank is outside u32",
        ),
        (
            "swap",
            "UPDATE process_samples_fast SET swap_bytes = -1 WHERE rank = 0",
            "swap_bytes is negative",
        ),
    ] {
        let fixture = TempDatabase::new(&format!("corrupt-{label}"));
        let store = fixture.store().await;
        let t = current_time_ms();
        let mut collected = snapshot(t);
        collected.processes = vec![ranked(10, 1, Some(1), Some(0), Some(0))];
        store
            .insert_snapshot(t, &collected)
            .await
            .expect("snapshot should insert");
        let pool = fixture.pool().await;
        sqlx::query(update)
            .execute(&pool)
            .await
            .expect("corrupting update");
        pool.close().await;

        let error = store
            .read_history(HistoryQuery {
                since_ms: Some(t),
                until_ms: Some(t),
                limit: Some(1),
            })
            .await
            .expect_err("a value outside its type must be refused")
            .to_string();
        assert!(error.contains(expected), "{label}: {error}");
    }
}

/// The INSERT a pre-0.14 daemon issues: the ten schema-v5 columns and nothing
/// for `swap_bytes`, `cpu_rank` or `memory_rank`. Run against a v6 table it
/// succeeds and leaves all three NULL (deep review 2026-10-09, finding 1).
async fn insert_old_shape_capture(
    pool: &SqlitePool,
    table: &str,
    captured_at_ms: i64,
    pids: &[i64],
) {
    sqlx::query(
        "INSERT OR IGNORE INTO process_commands (command_id, command) VALUES (900, 'old-daemon serve')",
    )
    .execute(pool)
    .await
    .expect("old daemon command row");
    let sql = match table {
        "process_samples_fast" => {
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, 900, ?, 1.5, 4096, 1, NULL, NULL)"
        }
        "process_samples" => {
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, 900, ?, 1.5, 4096, 1, NULL, NULL)"
        }
        other => panic!("unsupported table {other}"),
    };
    for (rank, pid) in pids.iter().enumerate() {
        let rank = i64::try_from(rank).expect("rank fits i64");
        sqlx::query(sql)
            .bind(captured_at_ms)
            .bind(rank)
            .bind(pid)
            .bind(90.0 - (rank as f64) * 10.0)
            .execute(pool)
            .await
            .expect("old-shape process row");
    }
}

/// `(pid, cpu_rank, memory_rank)` of a sample's processes, in the order read.
fn snapshot_ranks(processes: &[ProcessSnapshot]) -> Vec<(u32, Option<u32>, Option<u32>)> {
    processes
        .iter()
        .map(|process| (process.pid, process.cpu_rank, process.memory_rank))
        .collect()
}

/// `(rank, pid, swap_bytes, cpu_rank, memory_rank)` of the single capture a
/// process-history read is expected to return.
async fn process_route_ranks(
    store: &SqliteHistoryStore,
    since_ms: Option<i64>,
    until_ms: i64,
    expected_source: ProcessHistorySource,
    captured_at_ms: i64,
) -> Vec<StoredRanks> {
    let read = store
        .read_history_processes(HistoryQuery {
            since_ms,
            until_ms: Some(until_ms),
            limit: Some(10_000),
        })
        .await
        .expect("process history should read");
    assert_eq!(read.source, expected_source);
    let capture = read
        .captures
        .iter()
        .find(|capture| capture.captured_at_ms == captured_at_ms)
        .unwrap_or_else(|| panic!("{expected_source:?} read has no capture at {captured_at_ms}"));
    capture
        .processes
        .iter()
        .map(|process| {
            (
                process.rank,
                process.pid,
                process.swap_bytes,
                process.cpu_rank,
                process.memory_rank,
            )
        })
        .collect()
}

#[tokio::test]
async fn a_capture_with_no_ranks_written_by_an_old_daemon_into_a_v6_file_is_read_as_a_cpu_list() {
    // Break caught (ADR 0037): rows a still-running pre-0.14 daemon writes
    // into a migrated file come back in neither list; the rule is applied on
    // one read path and forgotten on another; or the read "repairs" the rows
    // on disk instead of leaving history as it was written.
    let fixture = TempDatabase::new("old-daemon-into-v6");
    let store = fixture.store().await;
    let fast_at = current_time_ms() - 10_000;
    let minute_at = fast_at + 3_000;
    for captured_at_ms in [fast_at, minute_at] {
        let mut metrics_only = snapshot(captured_at_ms);
        metrics_only.processes = Vec::new();
        store
            .insert_snapshot(captured_at_ms, &metrics_only)
            .await
            .expect("metric row should insert");
    }
    let pids = [501_i64, 502, 503, 504, 505];
    let pool = fixture.pool().await;
    // One capture per table, so each SELECT of `read_history` is the only
    // possible source of the rows it returns.
    insert_old_shape_capture(&pool, "process_samples_fast", fast_at, &pids).await;
    insert_old_shape_capture(&pool, "process_samples", minute_at, &pids).await;
    let on_disk: Vec<StoredRanks> = pids
        .iter()
        .enumerate()
        .map(|(rank, pid)| (rank as i64, *pid, None, None, None))
        .collect();
    assert_eq!(
        stored_ranks(&pool, "process_samples_fast", fast_at).await,
        on_disk,
        "fixture: the ten-column INSERT left both ranks NULL"
    );
    assert_eq!(
        stored_ranks(&pool, "process_samples", minute_at).await,
        on_disk
    );

    let as_cpu_list: Vec<(u32, Option<u32>, Option<u32>)> = (0_u32..5)
        .map(|rank| (501 + rank, Some(rank), None))
        .collect();
    for (label, captured_at_ms) in [("fast SELECT", fast_at), ("minute SELECT", minute_at)] {
        let history = store
            .read_history(HistoryQuery {
                since_ms: Some(captured_at_ms),
                until_ms: Some(captured_at_ms),
                limit: Some(1),
            })
            .await
            .expect("history should read");
        assert_eq!(history.len(), 1, "{label}");
        assert_eq!(
            snapshot_ranks(&history[0].snapshot.processes),
            as_cpu_list,
            "read_history, {label}: cpu_rank is the stored rank, and no memory rank is invented"
        );
        assert!(
            history[0]
                .snapshot
                .processes
                .iter()
                .all(|process| process.swap_bytes.is_none()),
            "{label}: swap stays unknown"
        );
    }
    let route_rows: Vec<StoredRanks> = pids
        .iter()
        .enumerate()
        .map(|(rank, pid)| (rank as i64, *pid, None, Some(rank as i64), None))
        .collect();
    assert_eq!(
        process_route_ranks(
            &store,
            Some(fast_at),
            fast_at,
            ProcessHistorySource::Fast,
            fast_at
        )
        .await,
        route_rows,
        "read_history_processes, fast table"
    );
    assert_eq!(
        process_route_ranks(
            &store,
            None,
            minute_at,
            ProcessHistorySource::Minute,
            minute_at
        )
        .await,
        route_rows,
        "read_history_processes, minute table"
    );

    // Nothing was written back: the rows are still the ones the old daemon wrote.
    assert_eq!(
        stored_ranks(&pool, "process_samples_fast", fast_at).await,
        on_disk
    );
    assert_eq!(
        stored_ranks(&pool, "process_samples", minute_at).await,
        on_disk
    );
    pool.close().await;
}

#[tokio::test]
async fn a_snapshot_without_ranks_is_stored_null_and_returned_as_a_cpu_list() {
    // Break caught: `insert_snapshot` hands back a sample that differs from
    // what a later read of the same rows returns, or the writer starts
    // inferring a rank at write time (ADR 0036 keeps the store recording what
    // it is given).
    let fixture = TempDatabase::new("rankless-snapshot");
    let store = fixture.store().await;
    let t = current_time_ms();
    let collected = snapshot(t);
    assert!(
        collected
            .processes
            .iter()
            .all(|process| process.cpu_rank.is_none() && process.memory_rank.is_none()),
        "fixture: the snapshot carries no ranks"
    );

    let stored = store
        .insert_snapshot(t, &collected)
        .await
        .expect("snapshot should insert");
    let as_cpu_list: Vec<(u32, Option<u32>, Option<u32>)> = (0_u32..4)
        .map(|rank| (40 + rank, Some(rank), None))
        .collect();
    assert_eq!(snapshot_ranks(&stored.snapshot.processes), as_cpu_list);
    let read = store
        .read_history(HistoryQuery {
            since_ms: Some(t),
            until_ms: Some(t),
            limit: Some(1),
        })
        .await
        .expect("history should read");
    assert_eq!(read, vec![stored]);

    let pool = fixture.pool().await;
    for table in ["process_samples_fast", "process_samples"] {
        assert_eq!(
            stored_ranks(&pool, table, t).await,
            expected_ranks(&collected.processes),
            "{table}: both ranks are NULL on disk"
        );
    }
    pool.close().await;
}

#[tokio::test]
async fn a_capture_in_which_any_row_has_a_rank_is_returned_exactly_as_stored() {
    // Break caught: the read-time rule is applied per row instead of per
    // capture (an unranked row beside ranked ones gets a CPU rank nobody
    // measured), or it tests `cpu_rank` alone and relabels a capture whose
    // rows are all in the memory list only.
    let cases: [(&str, Vec<ProcessSnapshot>); 3] = [
        (
            "mixed",
            vec![
                ranked(10, 100, Some(1), Some(0), Some(1)),
                ranked(11, 200, None, None, None),
                ranked(12, 900, Some(2), None, Some(0)),
                ranked(13, 300, None, None, None),
            ],
        ),
        (
            "memory-ranks-only",
            vec![
                ranked(20, 900, Some(1), None, Some(0)),
                ranked(21, 800, Some(2), None, Some(1)),
            ],
        ),
        (
            "one-ranked-row-last",
            vec![
                ranked(30, 100, None, None, None),
                ranked(31, 200, None, None, None),
                ranked(32, 300, None, Some(0), None),
            ],
        ),
    ];
    for (label, processes) in cases {
        let fixture = TempDatabase::new(&format!("as-stored-{label}"));
        let store = fixture.store().await;
        let t = current_time_ms();
        let mut collected = snapshot(t);
        collected.processes = processes.clone();

        let stored = store
            .insert_snapshot(t, &collected)
            .await
            .expect("snapshot should insert");
        assert_eq!(stored.snapshot.processes, processes, "{label}: insert");
        let read = store
            .read_history(HistoryQuery {
                since_ms: Some(t),
                until_ms: Some(t),
                limit: Some(1),
            })
            .await
            .expect("history should read");
        assert_eq!(
            read[0].snapshot.processes, processes,
            "{label}: read_history"
        );
        for (source, since_ms) in [
            (ProcessHistorySource::Fast, Some(t)),
            (ProcessHistorySource::Minute, None),
        ] {
            assert_eq!(
                process_route_ranks(&store, since_ms, t, source, t).await,
                expected_ranks(&processes),
                "{label}: read_history_processes {source:?}"
            );
        }
        let pool = fixture.pool().await;
        for table in ["process_samples_fast", "process_samples"] {
            assert_eq!(
                stored_ranks(&pool, table, t).await,
                expected_ranks(&processes),
                "{label}: {table} on disk"
            );
        }
        pool.close().await;
    }
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
            runnable: Some(1),
            total_threads: Some(2),
            last_pid: Some(3),
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
                gpu_percent: None,
                swap_bytes: None,
                cpu_rank: None,
                memory_rank: None,
            })
            .collect(),
        gpus: Vec::new(),
        sensors: Vec::new(),
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
