use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use tinytop_store::{SqliteHistoryStore, StoreError};

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
            "tinytop-migration-v3-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("fixture directory should be created");
        let path = dir.join("history.sqlite");
        let url = format!("sqlite://{}", path.display());
        Self { dir, url }
    }

    async fn raw_pool(&self) -> SqlitePool {
        let options = SqliteConnectOptions::from_str(&self.url)
            .expect("fixture URL should parse")
            .create_if_missing(true);
        SqlitePool::connect_with(options)
            .await
            .expect("fixture pool should connect")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

#[tokio::test]
async fn fresh_database_is_created_at_v3() {
    let fixture = TempDatabase::new("fresh");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh database should connect");
    assert_eq!(store.user_version().await.expect("version should read"), 3);
    store.close().await.expect("store should close");
}

#[tokio::test]
async fn v2_fixture_with_undecodable_json_refuses_and_leaves_the_file_untouched() {
    let fixture = TempDatabase::new("undecodable");
    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v2 fixture should be created")
        .close()
        .await
        .expect("fixture store should close");

    let pool = fixture.raw_pool().await;
    sqlx::query(
        r#"
        INSERT INTO metric_samples (
          sample_id, captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
          cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
          memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
          load_one, load_five, load_fifteen, load_percent, runnable_threads,
          total_threads, root_used_percent, snapshot_json
        ) VALUES (41, 1000, '2026-08-30T00:00:01Z', 'fixture', 'Linux',
                  1.0, 4, 2.0, 2, 100, 3.0, 3, 100,
                  0.1, 0.2, 0.3, 4.0, 1, 2, NULL, '{"not":"a snapshot"}')
        "#,
    )
    .execute(&pool)
    .await
    .expect("invalid JSON-bearing v2 row should seed");
    pool.close().await;

    let error = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect_err("undecodable JSON must refuse schema v3 migration");
    match error {
        StoreError::Migration { reason, remedy } => {
            assert!(reason.contains("metric_samples row 41"), "{reason}");
            assert!(reason.contains("does not decode"), "{reason}");
            assert!(remedy.contains("database was not modified"), "{remedy}");
        }
        other => panic!("expected migration refusal, observed {other:?}"),
    }

    let pool = fixture.raw_pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&pool)
            .await
            .expect("version should read"),
        2
    );
    let columns = sqlx::query("PRAGMA table_info(metric_samples)")
        .fetch_all(&pool)
        .await
        .expect("metric_samples shape should read");
    assert!(columns.iter().any(|row| row.get::<String, _>("name") == "snapshot_json"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'host_identity'",
        )
        .fetch_one(&pool)
        .await
        .expect("identity table count should read"),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated' AND json_extract(details_json, '$.toVersion') = 3",
        )
        .fetch_one(&pool)
        .await
        .expect("v3 marker count should read"),
        0
    );
    pool.close().await;
}
