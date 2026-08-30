use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value as JsonValue;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use tinytop_store::{
    SqliteHistoryStore,
    migration::{CREATE_SCHEMA_V2_SQL, CREATE_SCHEMA_V3_SQL},
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
            "tinytop-migration-v4-{label}-{}-{stamp}",
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
        "gpu_adapters" => "PRAGMA table_info(gpu_adapters)",
        "gpu_samples" => "PRAGMA table_info(gpu_samples)",
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

async fn apply_groups(pool: &SqlitePool, groups: impl IntoIterator<Item = &'static str>) {
    for group in groups {
        sqlx::raw_sql(group)
            .execute(pool)
            .await
            .expect("schema group");
    }
}

async fn seed_v3_process_rows(pool: &SqlitePool) {
    sqlx::query("INSERT INTO process_commands (command_id, command) VALUES (1, 'fixture')")
        .execute(pool)
        .await
        .expect("command row");
    for (rank, started_at) in [
        (0_i64, Some("2026-08-29T05:28:11Z")),
        (1, Some("-")),
        (2, Some("garbage")),
        (3, None),
    ] {
        sqlx::query(
            r#"
            INSERT INTO process_samples_fast (
              captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
              rss_bytes, parent_pid, started_at, gpu_percent
            ) VALUES (1000, ?, ?, 1, 1.0, 2.0, 3, NULL, ?, 4.0)
            "#,
        )
        .bind(rank)
        .bind(100_i64 + rank)
        .bind(started_at)
        .execute(pool)
        .await
        .expect("fast v3 process row");
        sqlx::query(
            r#"
            INSERT INTO process_samples (
              captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
              rss_bytes, parent_pid, started_at
            ) VALUES (1000, ?, ?, 1, 1.0, 2.0, 3, NULL, ?)
            "#,
        )
        .bind(rank)
        .bind(100_i64 + rank)
        .bind(started_at)
        .execute(pool)
        .await
        .expect("minute v3 process row");
    }
}

#[tokio::test]
async fn fresh_database_is_created_at_v4() {
    // Break caught: fresh databases retain v3 process text or omit the GPU tables.
    let fixture = TempDatabase::new("fresh");
    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh store")
        .close()
        .await
        .expect("close fresh store");
    let pool = fixture.raw_pool(false).await;

    assert_eq!(user_version(&pool).await, 4);
    for table in ["gpu_adapters", "gpu_samples"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("table existence"),
            1,
            "missing {table}"
        );
    }
    let columns = table_info(&pool, "process_samples_fast").await;
    assert_eq!(columns[8].1, "started_at_ms");
    assert_eq!(columns[8].2, "INTEGER");
    assert!(!columns.iter().any(|column| column.1 == "started_at"));
}

#[tokio::test]
async fn v3_fixture_migrates_to_v4_converting_started_at_and_creating_gpu_tables() {
    // Break caught: the rebuild drops rows, retains text, or silently coerces malformed text to zero.
    let fixture = TempDatabase::new("v3");
    let pool = fixture.raw_pool(true).await;
    apply_groups(&pool, CREATE_SCHEMA_V3_SQL).await;
    seed_v3_process_rows(&pool).await;
    pool.close().await;

    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v3 migration")
        .close()
        .await
        .expect("close migrated store");
    let pool = fixture.raw_pool(false).await;

    assert_eq!(user_version(&pool).await, 4);
    for table in ["process_samples_fast", "process_samples"] {
        let rows: Vec<Option<i64>> = sqlx::query_scalar(&format!(
            "SELECT started_at_ms FROM {table} ORDER BY rank"
        ))
        .fetch_all(&pool)
        .await
        .expect("converted start times");
        assert_eq!(rows, [Some(1_787_981_291_000), None, None, None]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .expect("rebuilt row count"),
            4
        );
    }
    let details: String = sqlx::query_scalar(
        "SELECT details_json FROM app_events WHERE label = 'SQLite schema migrated from v3 to v4'",
    )
    .fetch_one(&pool)
    .await
    .expect("v4 marker");
    let details: JsonValue = serde_json::from_str(&details).expect("marker JSON");
    assert_eq!(details["fromVersion"], 3);
    assert_eq!(details["toVersion"], 4);
    assert_eq!(details["fastRows"], 4);
    assert_eq!(details["minuteRows"], 4);
    assert_eq!(details["startedAtUnparsed"], 4);
    assert!(details["durationMs"].as_i64().is_some());
}

#[tokio::test]
async fn migrated_v4_schema_equals_a_fresh_v4_schema() {
    // Break caught: migration and fresh DDL disagree in column order, nullability, keys, or indexes.
    let migrated = TempDatabase::new("migrated-shape");
    let pool = migrated.raw_pool(true).await;
    apply_groups(&pool, CREATE_SCHEMA_V3_SQL).await;
    seed_v3_process_rows(&pool).await;
    pool.close().await;
    SqliteHistoryStore::connect(&migrated.url)
        .await
        .expect("migrated store")
        .close()
        .await
        .expect("close migrated store");

    let fresh = TempDatabase::new("fresh-shape");
    SqliteHistoryStore::connect(&fresh.url)
        .await
        .expect("fresh store")
        .close()
        .await
        .expect("close fresh store");

    let migrated_pool = migrated.raw_pool(false).await;
    let fresh_pool = fresh.raw_pool(false).await;
    for table in [
        "process_samples_fast",
        "process_samples",
        "gpu_adapters",
        "gpu_samples",
    ] {
        assert_eq!(
            table_info(&migrated_pool, table).await,
            table_info(&fresh_pool, table).await,
            "table_info differs for {table}"
        );
        assert_eq!(
            index_names(&migrated_pool, table).await,
            index_names(&fresh_pool, table).await,
            "index names differ for {table}"
        );
    }
}

#[tokio::test]
async fn a_v2_fixture_chains_to_v4() {
    // Break caught: the dispatcher stops after v2-to-v3 instead of completing v4.
    let fixture = TempDatabase::new("v2-chain");
    let pool = fixture.raw_pool(true).await;
    apply_groups(&pool, CREATE_SCHEMA_V2_SQL).await;
    pool.close().await;

    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v2 chain")
        .close()
        .await
        .expect("close chained store");
    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 4);
    assert_eq!(table_info(&pool, "process_samples_fast").await[8].1, "started_at_ms");
}

#[tokio::test]
async fn a_v0_fixture_chains_to_v4() {
    // Break caught: an uninitialized v0 file is created at an intermediate schema version.
    let fixture = TempDatabase::new("v0-chain");
    let pool = fixture.raw_pool(true).await;
    sqlx::query("PRAGMA user_version = 0")
        .execute(&pool)
        .await
        .expect("v0 marker");
    pool.close().await;

    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v0 chain")
        .close()
        .await
        .expect("close chained store");
    let pool = fixture.raw_pool(false).await;
    assert_eq!(user_version(&pool).await, 4);
    assert_eq!(table_info(&pool, "process_samples").await[8].1, "started_at_ms");
}
