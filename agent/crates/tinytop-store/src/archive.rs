use std::path::{Path, PathBuf};

use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};

use crate::{
    ArchiveErrorSource, SqliteHistoryStore, StoreError,
    ladder::{Tier, TierBucket},
    retention_ladder::ArchiveSettings,
    tier_bucket_from_row,
};

pub const ARCHIVE_BATCH_ROWS: usize = 1_000;
pub const MAX_ARCHIVE_BATCHES_PER_TICK: usize = 10;

const ARCHIVE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS metric_rollups_1h (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,
  min_cpu_usage_percent REAL NOT NULL,
  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL,
  min_memory_used_percent REAL NOT NULL,
  max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,
  min_swap_used_percent REAL NOT NULL,
  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,
  min_load_percent REAL NOT NULL,
  max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1h_newest
  ON metric_rollups_1h (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS archive_manifest (
  month TEXT PRIMARY KEY,
  exported_at_ms INTEGER NOT NULL,
  file TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  row_count INTEGER NOT NULL,
  bytes INTEGER NOT NULL
);

PRAGMA user_version = 1;
"#;

const ARCHIVE_COLUMNS: &str = r#"
  bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
  avg_cpu_usage_percent, min_cpu_usage_percent, max_cpu_usage_percent,
  avg_memory_used_percent, min_memory_used_percent, max_memory_used_percent,
  avg_swap_used_percent, min_swap_used_percent, max_swap_used_percent,
  avg_load_percent, min_load_percent, max_load_percent,
  avg_root_used_percent, min_root_used_percent, max_root_used_percent
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivePaths {
    pub db: PathBuf,
    pub directory: PathBuf,
}

pub fn archive_paths(main_db: &Path, settings: &ArchiveSettings) -> ArchivePaths {
    let directory = if settings.directory.is_empty() {
        main_db
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    } else {
        PathBuf::from(&settings.directory)
    };
    ArchivePaths {
        db: directory.join("history-archive.sqlite"),
        directory,
    }
}

pub async fn ensure_archive_schema(paths: &ArchivePaths) -> Result<(), StoreError> {
    std::fs::create_dir_all(&paths.directory).map_err(|source| StoreError::Archive {
        step: "create directory",
        source: ArchiveErrorSource::Io(source),
    })?;

    if archive_schema_is_current(paths).await? {
        return Ok(());
    }

    let options = SqliteConnectOptions::new()
        .filename(&paths.db)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|source| archive_sqlx("open", source))?;
    sqlx::raw_sql(ARCHIVE_SCHEMA_SQL)
        .execute(&mut connection)
        .await
        .map_err(|source| archive_sqlx("schema", source))?;
    connection
        .close()
        .await
        .map_err(|source| archive_sqlx("close", source))?;
    Ok(())
}

pub async fn move_expired_l4(
    store: &SqliteHistoryStore,
    paths: &ArchivePaths,
    cutoff_ms: i64,
    batch: usize,
) -> Result<i64, StoreError> {
    ensure_archive_schema(paths).await?;
    let batch = batch.min(ARCHIVE_BATCH_ROWS).max(1) as i64;
    let mut connection = store
        .pool
        .acquire()
        .await
        .map_err(|source| archive_sqlx("acquire", source))?;
    let archive_db = paths.db.to_string_lossy().into_owned();
    sqlx::query("ATTACH DATABASE ?1 AS archive")
        .bind(&archive_db)
        .execute(&mut *connection)
        .await
        .map_err(|source| archive_sqlx("attach", source))?;

    let result = move_attached_batch(&mut connection, cutoff_ms, batch).await;
    if let Err(source) = sqlx::query("DETACH DATABASE archive")
        .execute(&mut *connection)
        .await
    {
        connection.close_on_drop();
        if let Err(operation_error) = result {
            eprintln!("archive operation failed before detach also failed: {operation_error}");
        }
        return Err(archive_sqlx("detach", source));
    }
    result
}

pub async fn read_archive_points(
    paths: &ArchivePaths,
    since_ms: i64,
    until_ms: i64,
    limit: i64,
) -> Result<Vec<TierBucket>, StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "read open").await? else {
        return Ok(Vec::new());
    };
    let rows = sqlx::query(
        r#"
        SELECT
          bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
          avg_cpu_usage_percent, min_cpu_usage_percent, max_cpu_usage_percent,
          avg_memory_used_percent, min_memory_used_percent, max_memory_used_percent,
          avg_swap_used_percent, min_swap_used_percent, max_swap_used_percent,
          avg_load_percent, min_load_percent, max_load_percent,
          avg_root_used_percent, min_root_used_percent, max_root_used_percent
        FROM metric_rollups_1h
        WHERE newest_captured_at_ms >= ?1 AND newest_captured_at_ms <= ?2
        ORDER BY newest_captured_at_ms DESC
        LIMIT ?3
        "#,
    )
    .bind(since_ms)
    .bind(until_ms)
    .bind(limit.clamp(1, 10_000))
    .fetch_all(&mut connection)
    .await
    .map_err(|source| archive_sqlx("read", source))?;
    let mut buckets = rows
        .into_iter()
        .map(|row| {
            tier_bucket_from_row(row).map_err(|error| match error {
                StoreError::Sqlx(source) => archive_sqlx("read decode", source),
                other => other,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    buckets.reverse();
    Ok(buckets)
}

pub(super) async fn archive_coverage(
    paths: &ArchivePaths,
) -> Result<(i64, Option<i64>, Option<i64>), StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "coverage open").await? else {
        return Ok((0, None, None));
    };
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS bucket_count,
               MIN(bucket_start_ms) AS oldest_ms,
               MAX(bucket_start_ms) AS newest_ms
        FROM metric_rollups_1h
        "#,
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|source| archive_sqlx("coverage", source))?;
    Ok((
        row.try_get("bucket_count")
            .map_err(|source| archive_sqlx("coverage decode", source))?,
        row.try_get("oldest_ms")
            .map_err(|source| archive_sqlx("coverage decode", source))?,
        row.try_get("newest_ms")
            .map_err(|source| archive_sqlx("coverage decode", source))?,
    ))
}

async fn archive_schema_is_current(paths: &ArchivePaths) -> Result<bool, StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "schema inspect").await? else {
        return Ok(false);
    };
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut connection)
        .await
        .map_err(|source| archive_sqlx("schema inspect", source))?;
    if user_version != 1 {
        return Ok(false);
    }
    let object_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE (type = 'table' AND name IN ('metric_rollups_1h', 'archive_manifest'))
           OR (type = 'index' AND name = 'idx_metric_rollups_1h_newest')
        "#,
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|source| archive_sqlx("schema inspect", source))?;
    Ok(object_count == 3)
}

async fn open_archive_read_only(
    paths: &ArchivePaths,
    step: &'static str,
) -> Result<Option<SqliteConnection>, StoreError> {
    if !paths
        .db
        .try_exists()
        .map_err(|source| StoreError::Archive {
            step,
            source: ArchiveErrorSource::Io(source),
        })?
    {
        return Ok(None);
    }
    let options = SqliteConnectOptions::new()
        .filename(&paths.db)
        .read_only(true)
        .create_if_missing(false);
    SqliteConnection::connect_with(&options)
        .await
        .map(Some)
        .map_err(|source| archive_sqlx(step, source))
}

async fn move_attached_batch(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    cutoff_ms: i64,
    batch: i64,
) -> Result<i64, StoreError> {
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut **connection)
        .await
        .map_err(|source| archive_sqlx("begin", source))?;

    let row = match sqlx::query(
        r#"
        SELECT MIN(bucket_start_ms) AS min_ms,
               MAX(bucket_start_ms) AS max_ms,
               COUNT(*) AS row_count
        FROM (
          SELECT bucket_start_ms
          FROM main.metric_rollups_1h
          WHERE bucket_start_ms + ?1 <= ?2
          ORDER BY bucket_start_ms
          LIMIT ?3
        )
        "#,
    )
    .bind(Tier::L4.resolution_ms())
    .bind(cutoff_ms)
    .bind(batch)
    .fetch_one(&mut **connection)
    .await
    {
        Ok(row) => row,
        Err(source) => return rollback(connection, archive_sqlx("select", source)).await,
    };
    let row_count = match row.try_get::<i64, _>("row_count") {
        Ok(count) => count,
        Err(source) => return rollback(connection, archive_sqlx("select decode", source)).await,
    };
    if row_count == 0 {
        return match sqlx::query("COMMIT").execute(&mut **connection).await {
            Ok(_) => Ok(0),
            Err(source) => rollback(connection, archive_sqlx("commit", source)).await,
        };
    }
    let min_ms = match row.try_get::<i64, _>("min_ms") {
        Ok(value) => value,
        Err(source) => return rollback(connection, archive_sqlx("select decode", source)).await,
    };
    let max_ms = match row.try_get::<i64, _>("max_ms") {
        Ok(value) => value,
        Err(source) => return rollback(connection, archive_sqlx("select decode", source)).await,
    };

    // The main DB uses WAL, so SQLite does not promise atomic commits across the attached files.
    // INSERT OR IGNORE plus a count check makes a crash-duplicated batch idempotent and convergent.
    let insert_sql = format!(
        "INSERT OR IGNORE INTO archive.metric_rollups_1h ({ARCHIVE_COLUMNS}) SELECT {ARCHIVE_COLUMNS} FROM main.metric_rollups_1h WHERE bucket_start_ms BETWEEN ?1 AND ?2"
    );
    if let Err(source) = sqlx::query(sqlx::AssertSqlSafe(insert_sql))
        .bind(min_ms)
        .bind(max_ms)
        .execute(&mut **connection)
        .await
    {
        return rollback(connection, archive_sqlx("insert", source)).await;
    }

    let archived_count: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM archive.metric_rollups_1h WHERE bucket_start_ms BETWEEN ?1 AND ?2",
    )
    .bind(min_ms)
    .bind(max_ms)
    .fetch_one(&mut **connection)
    .await
    {
        Ok(count) => count,
        Err(source) => return rollback(connection, archive_sqlx("verify", source)).await,
    };
    if archived_count != row_count {
        let source = sqlx::Error::Protocol(format!(
            "archive batch count {archived_count} did not equal main batch count {row_count}"
        ));
        return rollback(connection, archive_sqlx("verify", source)).await;
    }

    let deleted = match sqlx::query(
        "DELETE FROM main.metric_rollups_1h WHERE bucket_start_ms BETWEEN ?1 AND ?2",
    )
    .bind(min_ms)
    .bind(max_ms)
    .execute(&mut **connection)
    .await
    {
        Ok(result) => result.rows_affected() as i64,
        Err(source) => return rollback(connection, archive_sqlx("delete", source)).await,
    };
    if deleted != row_count {
        let source = sqlx::Error::Protocol(format!(
            "deleted main batch count {deleted} did not equal selected batch count {row_count}"
        ));
        return rollback(connection, archive_sqlx("delete", source)).await;
    }

    if let Err(source) = sqlx::query("COMMIT").execute(&mut **connection).await {
        return rollback(connection, archive_sqlx("commit", source)).await;
    }

    let moved_until_ms = max_ms.saturating_add(Tier::L4.resolution_ms());
    if let Err(source) = sqlx::query(
        r#"
        INSERT INTO main.history_state (state_key, value_json, updated_at_ms)
        VALUES ('archiveMovedUntilMs', ?1, ?2)
        ON CONFLICT(state_key) DO UPDATE SET
          value_json = excluded.value_json,
          updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(moved_until_ms.to_string())
    .bind(moved_until_ms)
    .execute(&mut **connection)
    .await
    {
        return Err(archive_sqlx("watermark", source));
    }
    Ok(row_count)
}

async fn rollback(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    error: StoreError,
) -> Result<i64, StoreError> {
    if let Err(source) = sqlx::query("ROLLBACK").execute(&mut **connection).await {
        eprintln!("archive rollback failed after {error}: {source}");
    }
    Err(error)
}

fn archive_sqlx(step: &'static str, source: sqlx::Error) -> StoreError {
    StoreError::Archive {
        step,
        source: ArchiveErrorSource::Sqlx(source),
    }
}
