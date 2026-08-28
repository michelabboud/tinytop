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
use tinytop_store::{HistoryQuery, SqliteHistoryStore, StoreError};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

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

// Complete v0 schema from commit 07d3fcc, excluding metric_samples so each
// fixture can retain the runtime-specific metric_samples declaration above.
const V0_REMAINING_SCHEMA_DDL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
  ON metric_samples (runtime_kind, captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS app_settings (
  setting_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS metric_rollups_1m (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,
  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL,
  max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,
  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,
  max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1m_newest
  ON metric_rollups_1m (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);
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
async fn migrated_history_reads_only_rows_that_keep_snapshot_json() {
    // Break caught: raw history crosses the JSON retention boundary and tries
    // to deserialize migrated rows whose snapshot_json was intentionally nulled.
    let fixture = TempDatabase::new("migrated-history-json-window");
    let seeded = seed_v0_database(&fixture, RUST_V0_METRIC_SAMPLES_DDL).await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v0 database should migrate");
    let history = store
        .read_history(HistoryQuery {
            since_ms: None,
            until_ms: None,
            limit: Some(100),
        })
        .await
        .expect("raw history should stay within the JSON window");

    assert_eq!(history.len(), 7);
    assert_eq!(
        history
            .iter()
            .map(|sample| sample.captured_at_ms)
            .collect::<Vec<_>>(),
        (3_i64..10)
            .map(|index| seeded.now_ms - (9 - index) * TEN_MINUTES_MS)
            .collect::<Vec<_>>()
    );
    let latest = store
        .latest_snapshot()
        .await
        .expect("latest snapshot query")
        .expect("newest migrated row should keep JSON");
    assert_eq!(latest.captured_at_ms, seeded.now_ms);
}

#[tokio::test]
async fn complete_v0_schema_preserves_rollup_data_and_recreates_indexes() {
    // Break caught: migration is exercised only against metric_samples and can
    // silently skip the populated rollup ALTER path or lose legacy indexes/data.
    let fixture = TempDatabase::new("complete-v0-schema");
    let seeded = seed_v0_database(&fixture, RUST_V0_METRIC_SAMPLES_DDL).await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("complete v0 database should migrate");
    drop(store);

    let pool = verification_pool(&fixture.url).await;
    assert_complete_v0_schema_survived(&pool, seeded.now_ms).await;
    pool.close().await;
}

#[tokio::test]
async fn reconnect_completes_an_interrupted_post_schema_vacuum() {
    // Break caught: a v1 database with a committed but incomplete migration
    // audit permanently skips the required VACUUM on every later startup.
    let fixture = TempDatabase::new("resume-migration-vacuum");
    seed_v0_database(&fixture, RUST_V0_METRIC_SAMPLES_DDL).await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v0 database should migrate");
    drop(store);

    let pool = verification_pool(&fixture.url).await;
    sqlx::query(
        r#"
        UPDATE history_state
        SET value_json = json_set(value_json, '$.vacuumedAtMs', json('null'))
        WHERE state_key = 'schemaMigration'
        "#,
    )
    .execute(&pool)
    .await
    .expect("migration audit should be marked incomplete");
    sqlx::raw_sql(
        r#"
        CREATE TABLE reconnect_padding (payload BLOB NOT NULL);
        INSERT INTO reconnect_padding (payload) VALUES (zeroblob(1048576));
        DROP TABLE reconnect_padding;
        "#,
    )
    .execute(&pool)
    .await
    .expect("reconnect padding should create freelist pages");
    let freelist_before: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&pool)
        .await
        .expect("freelist count before reconnect");
    assert!(freelist_before > 0, "fixture should contain free pages");
    pool.close().await;

    let reopened = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v1 reconnect should complete the interrupted migration");
    drop(reopened);

    let pool = verification_pool(&fixture.url).await;
    let migration_json: String = sqlx::query_scalar(
        "SELECT value_json FROM history_state WHERE state_key = 'schemaMigration'",
    )
    .fetch_one(&pool)
    .await
    .expect("completed migration audit");
    let migration: JsonValue =
        serde_json::from_str(&migration_json).expect("valid completed migration JSON");
    assert!(migration["startedAtMs"].as_i64().is_some());
    assert!(migration["vacuumedAtMs"].as_i64().is_some());
    assert!(migration["bytesAfter"].as_i64().is_some());
    assert!(migration["durationMs"].as_i64().is_some());
    let marker_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated'")
            .fetch_one(&pool)
            .await
            .expect("schemaMigrated marker count after resumed completion");
    assert_eq!(marker_count, 1);
    let freelist_after: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&pool)
        .await
        .expect("freelist count after reconnect");
    assert_eq!(freelist_after, 0);
    pool.close().await;
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
    let pre_image_before =
        std::fs::metadata(&pre_image_path).expect("pre-existing pre-image metadata before refusal");
    let pre_image_length_before = pre_image_before.len();
    let pre_image_modified_before = pre_image_before
        .modified()
        .expect("pre-existing pre-image modification time before refusal");

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
    let pre_image_after =
        std::fs::metadata(&pre_image_path).expect("pre-existing pre-image metadata after refusal");
    assert_eq!(pre_image_after.len(), pre_image_length_before);
    assert_eq!(
        pre_image_after
            .modified()
            .expect("pre-existing pre-image modification time after refusal"),
        pre_image_modified_before
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
    sqlx::raw_sql(V0_REMAINING_SCHEMA_DDL)
        .execute(&pool)
        .await
        .expect("complete remaining v0 schema should be created");

    let wall_clock_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_millis() as i64;
    // Use the next whole minute as fixture "now" so the row exactly at the
    // inclusive 60-minute boundary cannot flake while connect obtains its clock.
    let now_ms = (wall_clock_ms.div_euclid(60_000) + 1) * 60_000;
    let snapshot_json =
        serde_json::to_string(&fixture_snapshot()).expect("fixture snapshot should serialize");
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
    sqlx::query(
        r#"
        INSERT INTO metric_rollups_1m (
          bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
          avg_cpu_usage_percent, max_cpu_usage_percent,
          avg_memory_used_percent, max_memory_used_percent,
          avg_swap_used_percent, max_swap_used_percent,
          avg_load_percent, max_load_percent, avg_root_used_percent
        ) VALUES (?, ?, ?, 40, 10.5, 11.5, 50.5, 51.5, 25.5, 26.5, 30.5, 31.5, 40.5)
        "#,
    )
    .bind(now_ms - 30 * 60_000)
    .bind(now_ms - 30 * 60_000 + 1_000)
    .bind(now_ms - 29 * 60_000 - 1)
    .execute(&pool)
    .await
    .expect("legacy rollup row should insert");
    sqlx::query(
        r#"
        INSERT INTO app_events (occurred_at_ms, marker_type, label, details_json)
        VALUES (?, 'daemonStart', 'legacy event', '{"source":"v0 fixture"}')
        "#,
    )
    .bind(now_ms - 5_000)
    .execute(&pool)
    .await
    .expect("legacy app event should insert");
    sqlx::query("PRAGMA user_version = 0")
        .execute(&pool)
        .await
        .expect("v0 user_version should be set");
    // This padding is a valid detector of a skipped VACUUM; JSON nulling is asserted separately.
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

fn fixture_snapshot() -> SystemSnapshot {
    SystemSnapshot {
        timestamp: "2026-08-28T00:00:00Z".to_string(),
        identity: IdentitySnapshot {
            hostname: "fixture".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Fixture Linux".to_string(),
            kernel: "fixture".to_string(),
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
            times: CpuTimes::default(),
        },
        memory: MemorySnapshot {
            total_bytes: 1_000,
            available_bytes: 500,
            used_bytes: 500,
            used_percent: 50.0,
        },
        swap: SwapSnapshot {
            total_bytes: 1_000,
            free_bytes: 750,
            used_bytes: 250,
            used_percent: 25.0,
        },
        load: LoadSnapshot {
            one: 1.0,
            five: 0.8,
            fifteen: 0.6,
            runnable: 2,
            total_threads: 100,
            last_pid: 42,
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: vec![FilesystemSnapshot {
            filesystem: "/dev/fixture".to_string(),
            fs_type: "ext4".to_string(),
            size_bytes: 1_000,
            used_bytes: 400,
            available_bytes: 600,
            used_percent: 40.0,
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
    assert_complete_v0_schema_survived(&pool, seeded.now_ms).await;

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

async fn assert_complete_v0_schema_survived(pool: &SqlitePool, now_ms: i64) {
    for column in [
        "min_cpu_usage_percent",
        "min_memory_used_percent",
        "min_swap_used_percent",
        "min_load_percent",
        "min_root_used_percent",
        "max_root_used_percent",
    ] {
        assert!(
            column_exists(pool, "metric_rollups_1m", column).await,
            "metric_rollups_1m should contain additive column {column}"
        );
    }

    let rollup = sqlx::query("SELECT * FROM metric_rollups_1m WHERE sample_count = 40")
        .fetch_one(pool)
        .await
        .expect("seeded legacy rollup should survive");
    assert_eq!(
        rollup.try_get::<i64, _>("bucket_start_ms").unwrap(),
        now_ms - 30 * 60_000
    );
    assert_eq!(
        rollup.try_get::<i64, _>("first_captured_at_ms").unwrap(),
        now_ms - 30 * 60_000 + 1_000
    );
    assert_eq!(
        rollup.try_get::<i64, _>("newest_captured_at_ms").unwrap(),
        now_ms - 29 * 60_000 - 1
    );
    assert_eq!(rollup.try_get::<i64, _>("sample_count").unwrap(), 40);
    for (column, expected) in [
        ("avg_cpu_usage_percent", 10.5),
        ("max_cpu_usage_percent", 11.5),
        ("avg_memory_used_percent", 50.5),
        ("max_memory_used_percent", 51.5),
        ("avg_swap_used_percent", 25.5),
        ("max_swap_used_percent", 26.5),
        ("avg_load_percent", 30.5),
        ("max_load_percent", 31.5),
        ("avg_root_used_percent", 40.5),
    ] {
        assert_eq!(
            rollup.try_get::<f64, _>(column).unwrap(),
            expected,
            "legacy value for {column} should survive"
        );
    }
    for column in [
        "min_cpu_usage_percent",
        "min_memory_used_percent",
        "min_swap_used_percent",
        "min_load_percent",
        "min_root_used_percent",
        "max_root_used_percent",
    ] {
        assert_eq!(
            rollup.try_get::<Option<f64>, _>(column).unwrap(),
            None,
            "additive column {column} should be NULL on the legacy row"
        );
    }

    let legacy_event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_events WHERE marker_type = 'daemonStart' AND label = 'legacy event'",
    )
    .fetch_one(pool)
    .await
    .expect("legacy event count");
    assert_eq!(legacy_event_count, 1);

    for index in [
        "idx_metric_samples_captured_at",
        "idx_metric_samples_runtime_captured_at",
        "idx_metric_rollups_1m_newest",
        "idx_metric_rollups_5m_newest",
        "idx_metric_rollups_1h_newest",
        "idx_fs_samples_mount_time",
        "idx_process_samples_time",
        "idx_app_events_occurred_type",
    ] {
        assert!(index_exists(pool, index).await, "missing index {index}");
    }
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

async fn index_exists(pool: &SqlitePool, index: &str) -> bool {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?")
            .bind(index)
            .fetch_one(pool)
            .await
            .expect("sqlite_master index query");
    count == 1
}
