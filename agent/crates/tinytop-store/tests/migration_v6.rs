//! Schema v6 (ADR 0036): `swap_bytes`, `cpu_rank` and `memory_rank` on both
//! process tables, added in place, with `cpu_rank = rank` backfilled.

use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value as JsonValue;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use tinytop_store::{
    SqliteHistoryStore, StoreError,
    migration::{
        CREATE_SCHEMA_V1_SQL, CREATE_SCHEMA_V2_SQL, CREATE_SCHEMA_V3_SQL, CREATE_SCHEMA_V4_SQL,
        CREATE_SCHEMA_V5_SQL, SCHEMA_VERSION,
    },
};

const V6_MARKER_LABEL: &str = "SQLite schema migrated from v5 to v6";

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
            "tinytop-migration-v6-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("fixture directory");
        Self {
            url: format!("sqlite://{}", dir.join("history.sqlite").display()),
            dir,
        }
    }

    async fn raw_pool(&self, create: bool) -> SqlitePool {
        SqlitePool::connect_with(
            SqliteConnectOptions::from_str(&self.url)
                .expect("fixture URL")
                .create_if_missing(create),
        )
        .await
        .expect("fixture pool")
    }

    /// Opens the file through the store, which runs the migration, and closes it.
    async fn migrate(&self) {
        SqliteHistoryStore::connect(&self.url)
            .await
            .expect("the store should open and migrate the fixture")
            .close()
            .await
            .expect("close migrated store");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

type TableInfoRow = (i64, String, String, i64, Option<String>, i64);

async fn table_info(pool: &SqlitePool, table: &str) -> Vec<TableInfoRow> {
    let sql = match table {
        "process_samples_fast" => "PRAGMA table_info(process_samples_fast)",
        "process_samples" => "PRAGMA table_info(process_samples)",
        other => panic!("unsupported table {other}"),
    };
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .expect("table info")
        .into_iter()
        .map(|row| {
            (
                row.get("cid"),
                row.get("name"),
                row.get("type"),
                row.get("notnull"),
                row.get("dflt_value"),
                row.get("pk"),
            )
        })
        .collect()
}

async fn index_names(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ? ORDER BY name",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("index names")
}

async fn user_version(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await
        .expect("user version")
}

async fn v6_marker_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated' AND label = ?",
    )
    .bind(V6_MARKER_LABEL)
    .fetch_one(pool)
    .await
    .expect("v6 marker count")
}

async fn apply_groups(pool: &SqlitePool, groups: impl IntoIterator<Item = &'static str>) {
    for group in groups {
        sqlx::raw_sql(group)
            .execute(pool)
            .await
            .expect("schema group");
    }
}

/// Every column a process row had before schema v6, with the reals as bit
/// patterns so "unchanged" means byte-identical rather than approximately equal.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OldColumns {
    captured_at_ms: i64,
    rank: i64,
    pid: i64,
    command_id: Option<i64>,
    cpu_percent_bits: u64,
    memory_percent_bits: u64,
    rss_bytes: i64,
    parent_pid: Option<i64>,
    started_at_ms: Option<i64>,
    gpu_percent_bits: Option<u64>,
}

async fn old_columns(pool: &SqlitePool, table: &str) -> Vec<OldColumns> {
    let sql = match table {
        "process_samples_fast" => {
            "SELECT captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent FROM process_samples_fast ORDER BY captured_at_ms, rank"
        }
        "process_samples" => {
            "SELECT captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent FROM process_samples ORDER BY captured_at_ms, rank"
        }
        other => panic!("unsupported table {other}"),
    };
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .expect("old columns")
        .into_iter()
        .map(|row| OldColumns {
            captured_at_ms: row.get("captured_at_ms"),
            rank: row.get("rank"),
            pid: row.get("pid"),
            command_id: row.get("command_id"),
            cpu_percent_bits: row.get::<f64, _>("cpu_percent").to_bits(),
            memory_percent_bits: row.get::<f64, _>("memory_percent").to_bits(),
            rss_bytes: row.get("rss_bytes"),
            parent_pid: row.get("parent_pid"),
            started_at_ms: row.get("started_at_ms"),
            gpu_percent_bits: row.get::<Option<f64>, _>("gpu_percent").map(f64::to_bits),
        })
        .collect()
}

/// `(rank, swap_bytes, cpu_rank, memory_rank)` of every row, in key order.
async fn new_columns(
    pool: &SqlitePool,
    table: &str,
) -> Vec<(i64, Option<i64>, Option<i64>, Option<i64>)> {
    let sql = match table {
        "process_samples_fast" => {
            "SELECT rank, swap_bytes, cpu_rank, memory_rank FROM process_samples_fast ORDER BY captured_at_ms, rank"
        }
        "process_samples" => {
            "SELECT rank, swap_bytes, cpu_rank, memory_rank FROM process_samples ORDER BY captured_at_ms, rank"
        }
        other => panic!("unsupported table {other}"),
    };
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .expect("new columns")
        .into_iter()
        .map(|row| {
            (
                row.get("rank"),
                row.get("swap_bytes"),
                row.get("cpu_rank"),
                row.get("memory_rank"),
            )
        })
        .collect()
}

/// After the migration every row that existed before it carries its old
/// `rank` as `cpu_rank`, and nothing in the two columns that were never
/// measured.
async fn assert_backfilled(pool: &SqlitePool, table: &str, expected_rows: usize) {
    let rows = new_columns(pool, table).await;
    assert_eq!(rows.len(), expected_rows, "{table} row count");
    for (rank, swap_bytes, cpu_rank, memory_rank) in rows {
        assert_eq!(cpu_rank, Some(rank), "{table} rank {rank}: cpu_rank");
        assert_eq!(swap_bytes, None, "{table} rank {rank}: swap is unknown");
        assert_eq!(
            memory_rank, None,
            "{table} rank {rank}: memory rank is unknown"
        );
    }
}

const SEED_CAPTURES: [i64; 3] = [1_000, 2_500, 61_000];
const SEED_ROWS_PER_CAPTURE: i64 = 4;
const SEED_ROWS: usize = 12;

/// Rows in the v4/v5 process-table shape, varied enough that a column swap,
/// a truncation or a default would change at least one of them.
async fn seed_v5_process_rows(pool: &SqlitePool) {
    for (command_id, command) in [(1_i64, "alpha --one"), (2, "beta --two")] {
        sqlx::query("INSERT INTO process_commands (command_id, command) VALUES (?, ?)")
            .bind(command_id)
            .bind(command)
            .execute(pool)
            .await
            .expect("command row");
    }
    for captured_at_ms in SEED_CAPTURES {
        for rank in 0..SEED_ROWS_PER_CAPTURE {
            let pid = 100 + captured_at_ms / 500 + rank;
            let cpu_percent = 0.1 + (rank as f64) / 3.0;
            let memory_percent = 12.345_678_901_234_5 / ((rank + 1) as f64);
            let rss_bytes = 9_007_199_254_740_993_i64 - rank;
            let parent_pid = (rank % 2 == 0).then_some(1_i64);
            let started_at_ms = (rank != 3).then_some(1_787_981_291_000_i64 + rank);
            let gpu_percent = (rank == 1).then_some(33.3_f64);
            sqlx::query(
                r#"
                INSERT INTO process_samples_fast (
                  captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
                  rss_bytes, parent_pid, started_at_ms, gpu_percent
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(captured_at_ms)
            .bind(rank)
            .bind(pid)
            .bind(1 + rank % 2)
            .bind(cpu_percent)
            .bind(memory_percent)
            .bind(rss_bytes)
            .bind(parent_pid)
            .bind(started_at_ms)
            .bind(gpu_percent)
            .execute(pool)
            .await
            .expect("fast process row");
            sqlx::query(
                r#"
                INSERT INTO process_samples (
                  captured_at_ms, rank, pid, cpu_percent, memory_percent, rss_bytes,
                  parent_pid, started_at_ms, command_id, gpu_percent
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(captured_at_ms)
            .bind(rank)
            .bind(pid)
            .bind(cpu_percent)
            .bind(memory_percent)
            .bind(rss_bytes)
            .bind(parent_pid)
            .bind(started_at_ms)
            .bind(2 - rank % 2)
            .bind(gpu_percent)
            .execute(pool)
            .await
            .expect("minute process row");
        }
    }
}

async fn seeded_v5(label: &str) -> TempDatabase {
    let fixture = TempDatabase::new(label);
    let pool = fixture.raw_pool(true).await;
    apply_groups(&pool, CREATE_SCHEMA_V5_SQL).await;
    seed_v5_process_rows(&pool).await;
    assert_eq!(user_version(&pool).await, 5);
    pool.close().await;
    fixture
}

#[tokio::test]
async fn schema_version_is_6() {
    // Break caught: the constant and the DDL's own `PRAGMA user_version` drift apart.
    assert_eq!(SCHEMA_VERSION, 6);
}

#[tokio::test]
async fn fresh_database_is_created_at_v6_with_the_three_columns_last() {
    // Break caught: a fresh file is created in the v5 shape and then "migrated"
    // (leaving a marker for a migration that never happened), or the columns
    // land anywhere but the end, where ADD COLUMN can never put them.
    let fixture = TempDatabase::new("fresh");
    fixture.migrate().await;
    let pool = fixture.raw_pool(false).await;

    assert_eq!(user_version(&pool).await, 6);
    assert_eq!(v6_marker_count(&pool).await, 0);
    for table in ["process_samples_fast", "process_samples"] {
        let columns = table_info(&pool, table).await;
        assert_eq!(columns.len(), 13, "{table}");
        for (offset, name) in ["swap_bytes", "cpu_rank", "memory_rank"]
            .into_iter()
            .enumerate()
        {
            let (_, column, data_type, not_null, default, primary_key) = &columns[10 + offset];
            assert_eq!(column, name, "{table}");
            assert_eq!(data_type, "INTEGER", "{table}.{name}");
            assert_eq!(*not_null, 0, "{table}.{name} is nullable");
            assert_eq!(*default, None, "{table}.{name} has no default");
            assert_eq!(*primary_key, 0, "{table}.{name} is not part of the key");
        }
        assert_eq!(columns[1].1, "rank");
        assert_eq!(columns[1].5, 2, "{table}.rank stays the key ordinal");
    }
    pool.close().await;
}

#[tokio::test]
async fn populated_v5_migrates_backfilling_cpu_rank_and_changing_nothing_else() {
    // Break caught: the backfill is omitted or writes the wrong column, swap or
    // memory rank are invented as zero, or a row or an old column is altered.
    let fixture = seeded_v5("populated").await;
    let pool = fixture.raw_pool(false).await;
    let mut before = Vec::new();
    for table in ["process_samples_fast", "process_samples"] {
        let rows = old_columns(&pool, table).await;
        assert_eq!(rows.len(), SEED_ROWS, "{table} fixture");
        before.push(rows);
    }
    pool.close().await;

    fixture.migrate().await;

    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 6);
    for (table, before) in ["process_samples_fast", "process_samples"]
        .into_iter()
        .zip(before)
    {
        assert_eq!(
            old_columns(&pool, table).await,
            before,
            "{table}: every pre-existing column of every row is unchanged"
        );
        assert_backfilled(&pool, table, SEED_ROWS).await;
    }
    let details: String = sqlx::query_scalar("SELECT details_json FROM app_events WHERE label = ?")
        .bind(V6_MARKER_LABEL)
        .fetch_one(&pool)
        .await
        .expect("v6 marker");
    let details: JsonValue = serde_json::from_str(&details).expect("marker JSON");
    assert_eq!(details["fromVersion"], 5);
    assert_eq!(details["toVersion"], 6);
    assert_eq!(details["fastRows"], 12);
    assert_eq!(details["minuteRows"], 12);
    assert!(details["durationMs"].as_i64().is_some());
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .expect("integrity check"),
        "ok"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .expect("foreign key check")
            .is_empty()
    );
    pool.close().await;
}

#[tokio::test]
async fn empty_v5_migrates_to_v6() {
    // Break caught: the row-count or verification guard mishandles zero rows.
    let fixture = TempDatabase::new("empty-v5");
    let pool = fixture.raw_pool(true).await;
    apply_groups(&pool, CREATE_SCHEMA_V5_SQL).await;
    pool.close().await;

    fixture.migrate().await;

    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 6);
    assert_eq!(v6_marker_count(&pool).await, 1);
    for table in ["process_samples_fast", "process_samples"] {
        assert_backfilled(&pool, table, 0).await;
        assert_eq!(table_info(&pool, table).await.len(), 13);
    }
    pool.close().await;
}

#[tokio::test]
async fn migrating_twice_changes_nothing_the_second_time() {
    // Break caught: a restart re-runs the ALTERs (duplicate column), writes a
    // second marker, or re-backfills rows written since at schema v6.
    let fixture = seeded_v5("idempotent").await;
    fixture.migrate().await;

    let pool = fixture.raw_pool(false).await;
    // A row the v6 writer could have produced: in the memory list only.
    sqlx::query(
        r#"
        INSERT INTO process_samples_fast (
          captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
          rss_bytes, swap_bytes, cpu_rank, memory_rank
        ) VALUES (70000, 5, 9, 1, 0.0, 1.0, 10, 2000, NULL, 0)
        "#,
    )
    .execute(&pool)
    .await
    .expect("v6 row");
    let mut first = Vec::new();
    for table in ["process_samples_fast", "process_samples"] {
        first.push((
            old_columns(&pool, table).await,
            new_columns(&pool, table).await,
            table_info(&pool, table).await,
        ));
    }
    pool.close().await;

    fixture.migrate().await;

    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 6);
    assert_eq!(v6_marker_count(&pool).await, 1);
    for (table, first) in ["process_samples_fast", "process_samples"]
        .into_iter()
        .zip(first)
    {
        assert_eq!(
            (
                old_columns(&pool, table).await,
                new_columns(&pool, table).await,
                table_info(&pool, table).await,
            ),
            first,
            "{table}"
        );
    }
    assert!(
        new_columns(&pool, "process_samples_fast")
            .await
            .contains(&(5, Some(2000), None, Some(0))),
        "a memory-only row written at v6 keeps its NULL cpu_rank across a restart"
    );
    pool.close().await;
}

#[tokio::test]
async fn a_failure_after_the_first_table_was_backfilled_leaves_the_v5_file_untouched() {
    // Break caught: the migration commits per table or per statement, so a
    // failure on the second table leaves the first one altered — or the file
    // reports v6 with a table that was never backfilled.
    let fixture = seeded_v5("interrupted").await;
    let pool = fixture.raw_pool(false).await;
    // `process_samples` is migrated second. Its backfill UPDATE aborts here,
    // after all three of its ALTERs and after `process_samples_fast` has been
    // altered and backfilled inside the same transaction.
    sqlx::query(
        "CREATE TRIGGER probe_abort_backfill BEFORE UPDATE ON process_samples BEGIN SELECT RAISE(ABORT, 'injected backfill failure'); END",
    )
    .execute(&pool)
    .await
    .expect("failure-injecting trigger");
    let mut before = Vec::new();
    for table in ["process_samples_fast", "process_samples"] {
        before.push((
            table_info(&pool, table).await,
            old_columns(&pool, table).await,
        ));
    }
    pool.close().await;

    let error = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect_err("the injected failure must refuse the migration");
    match error {
        StoreError::Migration { reason, remedy } => {
            assert!(
                reason.contains("`UPDATE process_samples SET cpu_rank = rank` failed"),
                "{reason}"
            );
            assert!(reason.contains("injected backfill failure"), "{reason}");
            assert!(remedy.contains("database was not modified"), "{remedy}");
        }
        other => panic!("expected migration refusal, observed {other:?}"),
    }

    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 5);
    assert_eq!(v6_marker_count(&pool).await, 0);
    for (table, before) in ["process_samples_fast", "process_samples"]
        .into_iter()
        .zip(before)
    {
        let columns = table_info(&pool, table).await;
        assert_eq!(columns.len(), 10, "{table} kept its v5 column count");
        assert_eq!(
            (columns, old_columns(&pool, table).await),
            before,
            "{table} shape and rows are exactly as before the attempt"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .expect("integrity check"),
        "ok"
    );

    // The refusal is not sticky: remove the cause and the same file migrates.
    sqlx::query("DROP TRIGGER probe_abort_backfill")
        .execute(&pool)
        .await
        .expect("trigger removal");
    pool.close().await;
    fixture.migrate().await;
    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 6);
    assert_eq!(v6_marker_count(&pool).await, 1);
    for table in ["process_samples_fast", "process_samples"] {
        assert_backfilled(&pool, table, SEED_ROWS).await;
    }
    pool.close().await;
}

#[tokio::test]
async fn a_v5_file_with_an_unrecognised_process_table_is_refused_untouched() {
    // Break caught: the migration alters a table it did not create — one that
    // already has a same-named column of unknown meaning, or has lost one —
    // or fails halfway through it with SQLite's bare "duplicate column name".
    for (label, tamper, table, found) in [
        (
            "extra-column",
            "ALTER TABLE process_samples ADD COLUMN memory_rank INTEGER",
            "process_samples",
            "gpu_percent, memory_rank]",
        ),
        (
            "missing-column",
            "ALTER TABLE process_samples_fast DROP COLUMN gpu_percent",
            "process_samples_fast",
            "started_at_ms]",
        ),
        (
            "missing-table",
            "DROP TABLE process_samples_fast",
            "process_samples_fast",
            "found []",
        ),
    ] {
        let fixture = seeded_v5(&format!("unrecognised-{label}")).await;
        let pool = fixture.raw_pool(false).await;
        sqlx::query(tamper).execute(&pool).await.expect("tamper");
        let minute_before = (
            table_info(&pool, "process_samples").await,
            old_columns(&pool, "process_samples").await,
        );
        pool.close().await;

        let error = SqliteHistoryStore::connect(&fixture.url)
            .await
            .expect_err("an unrecognised shape must be refused");
        match error {
            StoreError::Migration { reason, remedy } => {
                assert!(
                    reason.contains(&format!("schema v6 migration does not recognise {table}")),
                    "{label}: {reason}"
                );
                assert!(reason.contains(found), "{label}: {reason}");
                assert!(remedy.contains("./tinytop db backup"), "{label}: {remedy}");
                assert!(
                    remedy.contains("database was not modified"),
                    "{label}: {remedy}"
                );
            }
            other => panic!("{label}: expected migration refusal, observed {other:?}"),
        }

        let pool = fixture.raw_pool(false).await;
        assert_eq!(user_version(&pool).await, 5, "{label}");
        assert_eq!(v6_marker_count(&pool).await, 0, "{label}");
        assert_eq!(
            (
                table_info(&pool, "process_samples").await,
                old_columns(&pool, "process_samples").await,
            ),
            minute_before,
            "{label}: process_samples is exactly as the refusal found it"
        );
        if label != "missing-table" {
            assert!(
                !table_info(&pool, "process_samples_fast")
                    .await
                    .iter()
                    .any(|column| column.1 == "cpu_rank"),
                "{label}: nothing was added to process_samples_fast"
            );
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn migrated_v6_schema_equals_a_fresh_v6_schema() {
    // Break caught: ADD COLUMN and the fresh DDL disagree in column order,
    // type, nullability, default, keys, or indexes.
    let fresh = TempDatabase::new("fresh-shape");
    fresh.migrate().await;
    let fresh_pool = fresh.raw_pool(false).await;

    let migrated_v5 = seeded_v5("migrated-shape").await;
    migrated_v5.migrate().await;
    let migrated_v1 = TempDatabase::new("migrated-shape-v1");
    let pool = migrated_v1.raw_pool(true).await;
    sqlx::raw_sql(CREATE_SCHEMA_V1_SQL)
        .execute(&pool)
        .await
        .expect("v1 schema");
    pool.close().await;
    migrated_v1.migrate().await;

    for (label, migrated) in [("v5", &migrated_v5), ("v1", &migrated_v1)] {
        let migrated_pool = migrated.raw_pool(false).await;
        for table in ["process_samples_fast", "process_samples"] {
            assert_eq!(
                table_info(&migrated_pool, table).await,
                table_info(&fresh_pool, table).await,
                "table_info differs for {table} migrated from {label}"
            );
            assert_eq!(
                index_names(&migrated_pool, table).await,
                index_names(&fresh_pool, table).await,
                "index names differ for {table} migrated from {label}"
            );
        }
        let tables = "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name";
        assert_eq!(
            sqlx::query_scalar::<_, String>(tables)
                .fetch_all(&migrated_pool)
                .await
                .expect("migrated tables"),
            sqlx::query_scalar::<_, String>(tables)
                .fetch_all(&fresh_pool)
                .await
                .expect("fresh tables"),
            "table set differs migrated from {label}"
        );
        migrated_pool.close().await;
    }
    fresh_pool.close().await;
}

/// One fixture per earlier schema version: how to create it and how to give
/// it process rows in that version's own column set.
struct EarlierVersion {
    version: i64,
    schema: &'static [&'static str],
    /// `None`: the version has no process table to populate (v0).
    insert_minute: Option<&'static str>,
    insert_fast: Option<&'static str>,
}

const V0_MARKER_ONLY: [&str; 1] = ["PRAGMA user_version = 0"];
const V1_SCHEMA: [&str; 1] = [CREATE_SCHEMA_V1_SQL];

const EARLIER_VERSIONS: [EarlierVersion; 6] = [
    EarlierVersion {
        version: 0,
        schema: &V0_MARKER_ONLY,
        insert_minute: None,
        insert_fast: None,
    },
    EarlierVersion {
        version: 1,
        schema: &V1_SCHEMA,
        insert_minute: Some(
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at) VALUES (?, ?, ?, 'fixture --v1', 1.5, 2.5, 3, NULL, '2026-08-29T05:28:11Z')",
        ),
        insert_fast: None,
    },
    EarlierVersion {
        version: 2,
        schema: &CREATE_SCHEMA_V2_SQL,
        insert_minute: Some(
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, '2026-08-29T05:28:11Z')",
        ),
        insert_fast: Some(
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at, gpu_percent) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, '2026-08-29T05:28:11Z', NULL)",
        ),
    },
    EarlierVersion {
        version: 3,
        schema: &CREATE_SCHEMA_V3_SQL,
        insert_minute: Some(
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, '2026-08-29T05:28:11Z')",
        ),
        insert_fast: Some(
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at, gpu_percent) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, '2026-08-29T05:28:11Z', 4.0)",
        ),
    },
    EarlierVersion {
        version: 4,
        schema: &CREATE_SCHEMA_V4_SQL,
        insert_minute: Some(
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, 1787981291000, 4.0)",
        ),
        insert_fast: Some(
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, 1787981291000, 4.0)",
        ),
    },
    EarlierVersion {
        version: 5,
        schema: &CREATE_SCHEMA_V5_SQL,
        insert_minute: Some(
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, 1787981291000, 4.0)",
        ),
        insert_fast: Some(
            "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, ?, ?, 1, 1.5, 2.5, 3, NULL, 1787981291000, 4.0)",
        ),
    },
];

/// `(captured_at_ms, rank, pid)` of the rows seeded into each process table:
/// two captures, three and two rows, ranks starting at zero as the Rust
/// writer has always numbered them.
const CHAIN_ROWS: [(i64, i64, i64); 5] = [
    (1_000, 0, 10),
    (1_000, 1, 11),
    (1_000, 2, 12),
    (61_000, 0, 20),
    (61_000, 1, 21),
];

async fn chain_keys(pool: &SqlitePool, table: &str) -> Vec<(i64, i64, i64)> {
    let sql = match table {
        "process_samples_fast" => {
            "SELECT captured_at_ms, rank, pid FROM process_samples_fast ORDER BY captured_at_ms, rank"
        }
        _ => "SELECT captured_at_ms, rank, pid FROM process_samples ORDER BY captured_at_ms, rank",
    };
    sqlx::query_as(sql).fetch_all(pool).await.expect("row keys")
}

#[tokio::test]
async fn every_earlier_schema_version_migrates_to_v6_empty_and_populated() {
    // Break caught: the dispatcher's chain for some starting version stops
    // short of v6, or an older step hands v5→v6 a shape it then refuses, or
    // rows carried up from an older version miss the backfill.
    for earlier in &EARLIER_VERSIONS {
        for populated in [false, true] {
            let version = earlier.version;
            let label = format!("chain-v{version}-{populated}");
            let fixture = TempDatabase::new(&label);
            let pool = fixture.raw_pool(true).await;
            apply_groups(&pool, earlier.schema.iter().copied()).await;
            assert_eq!(user_version(&pool).await, version, "{label} fixture");
            let mut expected_minute = 0;
            let mut expected_fast = 0;
            if populated {
                if version >= 2 {
                    sqlx::query(
                        "INSERT INTO process_commands (command_id, command) VALUES (1, 'fixture')",
                    )
                    .execute(&pool)
                    .await
                    .expect("command row");
                }
                for (insert, expected) in [
                    (earlier.insert_minute, &mut expected_minute),
                    (earlier.insert_fast, &mut expected_fast),
                ] {
                    let Some(insert) = insert else { continue };
                    for (captured_at_ms, rank, pid) in CHAIN_ROWS {
                        sqlx::query(insert)
                            .bind(captured_at_ms)
                            .bind(rank)
                            .bind(pid)
                            .execute(&pool)
                            .await
                            .unwrap_or_else(|error| panic!("{label} process row: {error}"));
                    }
                    *expected = CHAIN_ROWS.len();
                }
            }
            pool.close().await;

            fixture.migrate().await;

            let pool = fixture.raw_pool(false).await;
            assert_eq!(user_version(&pool).await, 6, "{label}");
            // A fresh (v0, no tables) file is created at v6, not migrated to it.
            assert_eq!(
                v6_marker_count(&pool).await,
                i64::from(version != 0),
                "{label}"
            );
            for (table, expected) in [
                ("process_samples", expected_minute),
                ("process_samples_fast", expected_fast),
            ] {
                assert_eq!(table_info(&pool, table).await.len(), 13, "{label} {table}");
                assert_backfilled(&pool, table, expected).await;
                if expected > 0 {
                    assert_eq!(
                        chain_keys(&pool, table).await,
                        CHAIN_ROWS,
                        "{label} {table}"
                    );
                }
            }
            pool.close().await;
        }
    }
}

#[tokio::test]
#[ignore = "timing run: TINYTOP_V6_TIMING_ROWS=<rows per process table> (default 400000); prints the measured migration time"]
async fn populated_v5_migration_time_at_a_live_row_count() {
    let rows_per_table: i64 = std::env::var("TINYTOP_V6_TIMING_ROWS")
        .ok()
        .map(|value| value.parse().expect("TINYTOP_V6_TIMING_ROWS is an integer"))
        .unwrap_or(400_000);
    let fixture = TempDatabase::new("timing");
    let pool = fixture.raw_pool(true).await;
    apply_groups(&pool, CREATE_SCHEMA_V5_SQL).await;
    sqlx::query("INSERT INTO process_commands (command_id, command) VALUES (1, 'fixture')")
        .execute(&pool)
        .await
        .expect("command row");
    // Eight rows per capture, the count the live database held before 0.12.1.
    for insert in [
        "WITH RECURSIVE seq(n) AS (VALUES(0) UNION ALL SELECT n + 1 FROM seq WHERE n + 1 < ?) INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) SELECT 1787000000000 + (n / 8) * 1500, n % 8, 1000 + n % 4096, 1, (n % 997) / 9.97, (n % 113) / 1.13, 4096 * (n % 100000), 1, 1787000000000 - n, NULL FROM seq",
        "WITH RECURSIVE seq(n) AS (VALUES(0) UNION ALL SELECT n + 1 FROM seq WHERE n + 1 < ?) INSERT INTO process_samples (captured_at_ms, rank, pid, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, command_id, gpu_percent) SELECT 1787000000000 + (n / 8) * 60000, n % 8, 1000 + n % 4096, (n % 997) / 9.97, (n % 113) / 1.13, 4096 * (n % 100000), 1, 1787000000000 - n, 1, NULL FROM seq",
    ] {
        sqlx::query(insert)
            .bind(rows_per_table)
            .execute(&pool)
            .await
            .expect("bulk process rows");
    }
    pool.close().await;
    let bytes_before = fs::metadata(fixture.dir.join("history.sqlite"))
        .expect("fixture metadata")
        .len();

    let started = Instant::now();
    fixture.migrate().await;
    let connect_ms = started.elapsed().as_millis();

    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 6);
    let details: String = sqlx::query_scalar("SELECT details_json FROM app_events WHERE label = ?")
        .bind(V6_MARKER_LABEL)
        .fetch_one(&pool)
        .await
        .expect("v6 marker");
    let unverified: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM process_samples_fast WHERE cpu_rank IS NOT rank) + (SELECT COUNT(*) FROM process_samples WHERE cpu_rank IS NOT rank)",
    )
    .fetch_one(&pool)
    .await
    .expect("verification count");
    assert_eq!(unverified, 0);
    pool.close().await;
    let bytes_after = fs::metadata(fixture.dir.join("history.sqlite"))
        .expect("fixture metadata")
        .len();
    println!(
        "v5 -> v6 with {rows_per_table} rows per process table: marker {details}; connect+migrate+close {connect_ms} ms; main file {bytes_before} -> {bytes_after} bytes"
    );
}
