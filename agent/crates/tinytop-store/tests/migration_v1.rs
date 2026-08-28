use std::{
    path::PathBuf,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value as JsonValue;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tinytop_store::{SqliteHistoryStore, StoreError};

const TEN_MINUTES_MS: i64 = 10 * 60 * 1_000;
const SNAPSHOT_JSON_KEEP_MS: i64 = 60 * 60 * 1_000;

// Copied verbatim from the v0 Rust store schema.
const RUST_V0_METRIC_SAMPLES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS metric_samples (
  sample_id INTEGER PRIMARY KEY,
  captured_at_ms INTEGER NOT NULL UNIQUE,
  snapshot_timestamp TEXT NOT NULL,
  hostname TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  cpu_usage_percent REAL NOT NULL,
  cpu_cores INTEGER NOT NULL,
  memory_used_percent REAL NOT NULL,
  memory_used_bytes INTEGER NOT NULL,
  memory_total_bytes INTEGER NOT NULL,
  swap_used_percent REAL NOT NULL,
  swap_used_bytes INTEGER NOT NULL,
  swap_total_bytes INTEGER NOT NULL,
  load_one REAL NOT NULL,
  load_five REAL NOT NULL,
  load_fifteen REAL NOT NULL,
  load_percent REAL NOT NULL,
  runnable_threads INTEGER NOT NULL,
  total_threads INTEGER NOT NULL,
  root_used_percent REAL,
  snapshot_json TEXT NOT NULL
)
"#;

// Copied from src/history-store.ts; the Bun runtime also creates v0 with
// snapshot_json constrained NOT NULL.
const BUN_V0_METRIC_SAMPLES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS metric_samples (
  sample_id INTEGER PRIMARY KEY,
  captured_at_ms INTEGER NOT NULL UNIQUE,
  snapshot_timestamp TEXT NOT NULL,
  hostname TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  cpu_usage_percent REAL NOT NULL,
  cpu_cores INTEGER NOT NULL,
  memory_used_percent REAL NOT NULL,
  memory_used_bytes INTEGER NOT NULL,
  memory_total_bytes INTEGER NOT NULL,
  swap_used_percent REAL NOT NULL,
  swap_used_bytes INTEGER NOT NULL,
  swap_total_bytes INTEGER NOT NULL,
  load_one REAL NOT NULL,
  load_five REAL NOT NULL,
  load_fifteen REAL NOT NULL,
  load_percent REAL NOT NULL,
  runnable_threads INTEGER NOT NULL,
  total_threads INTEGER NOT NULL,
  root_used_percent REAL,
  snapshot_json TEXT NOT NULL
)
"#;

struct TempDatabase {
    dir: PathBuf,
    path: PathBuf,
    url: String,
}

impl TempDatabase {
    fn new(prefix: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("tinytop-{prefix}-{}-{stamp}", std::process::id()));
        assert!(dir.starts_with(std::env::temp_dir()));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("history.sqlite");
        let url = format!("sqlite://{}", path.display());
        Self { dir, path, url }
    }

    fn pre_image_path(&self) -> PathBuf {
        let mut path = self.path.as_os_str().to_os_string();
        path.push(".pre-v0.sqlite");
        PathBuf::from(path)
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

struct SeededV0 {
    now_ms: i64,
    bytes_before: u64,
}

#[tokio::test]
async fn fresh_database_is_created_at_schema_version_1() {
    // Break caught: connecting to a new file leaves user_version at 0 or omits
    // any v1 table, index, additive column, or nullable snapshot_json contract.
    let fixture = TempDatabase::new("fresh-v1");

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh database should connect");
    drop(store);

    let pool = verification_pool(&fixture.url).await;
    assert_eq!(schema_version(&pool).await, 1);
    for table in [
        "metric_rollups_5m",
        "metric_rollups_1h",
        "history_state",
        "fs_samples",
        "process_samples",
    ] {
        assert!(table_exists(&pool, table).await, "missing table {table}");
    }
    assert!(
        column_exists(&pool, "metric_rollups_1m", "min_cpu_usage_percent").await,
        "metric_rollups_1m should contain min_cpu_usage_percent"
    );
    let snapshot_json_not_null: i64 = sqlx::query(
        "SELECT [notnull] FROM pragma_table_info('metric_samples') WHERE name = 'snapshot_json'",
    )
    .fetch_one(&pool)
    .await
    .expect("snapshot_json schema row")
    .try_get("notnull")
    .expect("snapshot_json notnull flag");
    assert_eq!(snapshot_json_not_null, 0);
    pool.close().await;
}

#[tokio::test]
async fn v0_database_is_migrated_with_pre_image_and_json_window() {
    // Break caught: a populated v0 database is altered without a complete
    // pre-image, loses rows, keeps JSON outside the window, or omits its audit record.
    let fixture = TempDatabase::new("migrate-v0");
    let seeded = seed_v0_database(&fixture, RUST_V0_METRIC_SAMPLES_DDL).await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v0 database should migrate");
    drop(store);

    verify_successful_migration(&fixture, &seeded).await;
}

#[tokio::test]
async fn migration_refuses_when_pre_image_exists() {
    // Break caught: migration overwrites, deletes, or silently skips an existing
    // pre-image instead of failing closed before touching the v0 database.
    let fixture = TempDatabase::new("refuse-pre-image");
    seed_v0_database(&fixture, RUST_V0_METRIC_SAMPLES_DDL).await;
    let database_before = std::fs::read(&fixture.path).expect("read v0 database bytes");
    let pre_image_path = fixture.pre_image_path();
    std::fs::File::create(&pre_image_path).expect("pre-existing pre-image fixture");

    let error = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect_err("migration should refuse an existing pre-image");

    match error {
        StoreError::Migration { reason, remedy } => {
            assert!(
                reason.contains(&pre_image_path.display().to_string()),
                "reason should name the pre-image path: {reason}"
            );
            assert!(
                remedy.to_ascii_lowercase().contains("move"),
                "remedy should tell the operator to move the pre-image: {remedy}"
            );
        }
        other => panic!("expected StoreError::Migration, got {other}"),
    }

    assert_eq!(
        std::fs::read(&fixture.path).expect("read refused v0 database bytes"),
        database_before,
        "a refused migration must leave the v0 database byte-identical"
    );
    let pool = verification_pool(&fixture.url).await;
    assert_eq!(schema_version(&pool).await, 0);
    pool.close().await;
}

#[tokio::test]
async fn bun_created_database_migrates_the_same_way() {
    // Break caught: migration accepts the Rust-created v0 shape but fails on
    // Bun's explicit snapshot_json TEXT NOT NULL database.
    let fixture = TempDatabase::new("migrate-bun-v0");
    let seeded = seed_v0_database(&fixture, BUN_V0_METRIC_SAMPLES_DDL).await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("Bun-created v0 database should migrate");
    drop(store);

    verify_successful_migration(&fixture, &seeded).await;
}

async fn seed_v0_database(fixture: &TempDatabase, metric_samples_ddl: &'static str) -> SeededV0 {
    let options = SqliteConnectOptions::from_str(&fixture.url)
        .expect("valid SQLite URL")
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("v0 fixture database should connect");
    for pragma in [
        "PRAGMA journal_mode = WAL",
        "PRAGMA synchronous = NORMAL",
        "PRAGMA busy_timeout = 5000",
        "PRAGMA foreign_keys = ON",
    ] {
        sqlx::query(pragma)
            .execute(&pool)
            .await
            .expect("v0 fixture pragma should apply");
    }
    sqlx::query(metric_samples_ddl)
        .execute(&pool)
        .await
        .expect("v0 metric_samples table should be created");

    let wall_clock_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_millis() as i64;
    // Use the next whole minute as fixture "now" so the row exactly at the
    // inclusive 60-minute boundary cannot flake while connect obtains its clock.
    let now_ms = (wall_clock_ms.div_euclid(60_000) + 1) * 60_000;
    let snapshot_json = format!("\"{}\"", "x".repeat(1_022));
    assert_eq!(snapshot_json.len(), 1_024);
    for index in 0..10_i64 {
        let captured_at_ms = now_ms - (9 - index) * TEN_MINUTES_MS;
        sqlx::query(
            r#"
            INSERT INTO metric_samples (
              captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
              cpu_usage_percent, cpu_cores,
              memory_used_percent, memory_used_bytes, memory_total_bytes,
              swap_used_percent, swap_used_bytes, swap_total_bytes,
              load_one, load_five, load_fifteen, load_percent,
              runnable_threads, total_threads, root_used_percent, snapshot_json
            ) VALUES (
              ?, '2026-08-28T00:00:00Z', 'fixture', 'Linux',
              10.0, 4,
              50.0, 500, 1000,
              25.0, 250, 1000,
              1.0, 0.8, 0.6, 25.0,
              2, 100, 40.0, ?
            )
            "#,
        )
        .bind(captured_at_ms)
        .bind(&snapshot_json)
        .execute(&pool)
        .await
        .expect("v0 fixture row should insert");
    }
    sqlx::query("PRAGMA user_version = 0")
        .execute(&pool)
        .await
        .expect("v0 user_version should be set");
    // A compact ten-row v0 file is smaller than the five new v1 tables and
    // indexes, so it cannot prove that the required post-migration VACUUM ran.
    // Leave deterministic freelist pages without changing the ten-row logical
    // fixture; skipping VACUUM will then keep the file larger than `bytes_after`.
    sqlx::query("CREATE TABLE migration_padding (payload BLOB NOT NULL)")
        .execute(&pool)
        .await
        .expect("migration padding table should be created");
    sqlx::query("INSERT INTO migration_padding (payload) VALUES (zeroblob(1048576))")
        .execute(&pool)
        .await
        .expect("migration padding row should insert");
    sqlx::query("DROP TABLE migration_padding")
        .execute(&pool)
        .await
        .expect("migration padding table should be dropped");
    pool.close().await;

    let bytes_before = std::fs::metadata(&fixture.path)
        .expect("v0 database metadata")
        .len();
    SeededV0 {
        now_ms,
        bytes_before,
    }
}

async fn verify_successful_migration(fixture: &TempDatabase, seeded: &SeededV0) {
    let pre_image_path = fixture.pre_image_path();
    assert!(pre_image_path.exists(), "migration pre-image should exist");

    let pre_image_url = format!("sqlite://{}", pre_image_path.display());
    let pre_image_pool = verification_pool(&pre_image_url).await;
    let pre_image_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples")
        .fetch_one(&pre_image_pool)
        .await
        .expect("pre-image row count");
    assert_eq!(pre_image_rows, 10);
    assert_eq!(schema_version(&pre_image_pool).await, 0);
    pre_image_pool.close().await;

    let pool = verification_pool(&fixture.url).await;
    assert_eq!(schema_version(&pool).await, 1);
    let cutoff_ms = seeded.now_ms - SNAPSHOT_JSON_KEEP_MS;
    let recent_json_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM metric_samples WHERE captured_at_ms >= ? AND snapshot_json IS NOT NULL",
    )
    .bind(cutoff_ms)
    .fetch_one(&pool)
    .await
    .expect("recent JSON row count");
    let old_null_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM metric_samples WHERE captured_at_ms < ? AND snapshot_json IS NULL",
    )
    .bind(cutoff_ms)
    .fetch_one(&pool)
    .await
    .expect("old NULL row count");
    assert_eq!(recent_json_rows, 7);
    assert_eq!(old_null_rows, 3);

    let migration_json: String = sqlx::query_scalar(
        "SELECT value_json FROM history_state WHERE state_key = 'schemaMigration'",
    )
    .fetch_one(&pool)
    .await
    .expect("schemaMigration history state");
    let migration: JsonValue =
        serde_json::from_str(&migration_json).expect("valid schemaMigration JSON");
    assert_eq!(migration["from"], 0);
    assert_eq!(migration["to"], 1);
    assert_eq!(
        migration["preImagePath"],
        pre_image_path.display().to_string()
    );
    let duration_ms = migration["durationMs"]
        .as_i64()
        .expect("migration durationMs");
    eprintln!("10-row fixture migration duration: {duration_ms} ms");

    let marker_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated'")
            .fetch_one(&pool)
            .await
            .expect("schemaMigrated marker count");
    assert_eq!(marker_count, 1);
    pool.close().await;

    let bytes_after = std::fs::metadata(&fixture.path)
        .expect("migrated database metadata")
        .len();
    assert!(
        bytes_after < seeded.bytes_before,
        "post-migration VACUUM should shrink the fixture: before={} after={bytes_after}",
        seeded.bytes_before
    );
}

async fn verification_pool(database_url: &str) -> SqlitePool {
    SqlitePool::connect(database_url)
        .await
        .expect("verification pool should connect")
}

async fn schema_version(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await
        .expect("PRAGMA user_version")
}

async fn table_exists(pool: &SqlitePool, table: &str) -> bool {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(table)
            .fetch_one(pool)
            .await
            .expect("sqlite_master table query");
    count == 1
}

async fn column_exists(pool: &SqlitePool, table: &str, column: &str) -> bool {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
        .bind(table)
        .bind(column)
        .fetch_one(pool)
        .await
        .expect("pragma_table_info column query");
    count == 1
}
