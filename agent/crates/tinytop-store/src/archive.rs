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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveBatch {
    pub min_ms: i64,
    pub max_ms: i64,
    pub row_count: i64,
}

pub(crate) fn archive_remedy(step: &'static str) -> &'static str {
    match step {
        "watermark" => {
            "the batch is committed in history-archive.sqlite and removed from the main database; only the watermark bookkeeping failed — retrying is safe, nothing is duplicated or lost"
        }
        "detach" => {
            "the batch is committed in history-archive.sqlite and removed from the main database; only the detach bookkeeping failed — retrying is safe, nothing is duplicated or lost"
        }
        _ => {
            "keep history-archive.sqlite and the main database unchanged, check the archive directory is writable, and retry — nothing was deleted from the main database"
        }
    }
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

#[derive(Clone, Copy)]
struct ArchiveSchemaInspection {
    user_version: i64,
    object_count: i64,
    required_object_count: i64,
}

impl ArchiveSchemaInspection {
    fn accepted(self) -> bool {
        self.user_version == 1 || (self.user_version == 0 && self.object_count == 0)
    }

    fn current(self) -> bool {
        self.user_version == 1 && self.required_object_count == 3
    }
}

async fn inspect_archive_schema(
    connection: &mut SqliteConnection,
) -> Result<ArchiveSchemaInspection, StoreError> {
    let user_version = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *connection)
        .await
        .map_err(|source| archive_sqlx("schema", source))?;
    let object_count = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master")
        .fetch_one(&mut *connection)
        .await
        .map_err(|source| archive_sqlx("schema", source))?;
    let required_object_count = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE (type = 'table' AND name IN ('metric_rollups_1h', 'archive_manifest'))
           OR (type = 'index' AND name = 'idx_metric_rollups_1h_newest')
        "#,
    )
    .fetch_one(connection)
    .await
    .map_err(|source| archive_sqlx("schema", source))?;
    Ok(ArchiveSchemaInspection {
        user_version,
        object_count,
        required_object_count,
    })
}

fn archive_schema_refusal(
    paths: &ArchivePaths,
    inspection: ArchiveSchemaInspection,
) -> Option<StoreError> {
    (!inspection.accepted()).then(|| StoreError::Archive {
        step: "schema",
        source: ArchiveErrorSource::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "history-archive.sqlite at {} has user_version {} with {} objects; expected a tinytop archive (user_version 1) — move the file away or point retentionLadder.archive.directory elsewhere",
                paths.db.display(), inspection.user_version, inspection.object_count
            ),
        )),
    })
}

pub async fn ensure_archive_schema(paths: &ArchivePaths) -> Result<(), StoreError> {
    std::fs::create_dir_all(&paths.directory).map_err(|source| StoreError::Archive {
        step: "create directory",
        source: ArchiveErrorSource::Io(source),
    })?;

    if let Some(mut inspection_connection) = open_archive_read_only(paths, "open").await? {
        let inspection = inspect_archive_schema(&mut inspection_connection).await?;
        if let Some(error) = archive_schema_refusal(paths, inspection) {
            if let Err(source) = inspection_connection.close().await {
                eprintln!("archive step close failed while refusing schema: {source}");
            }
            return Err(error);
        }
        inspection_connection
            .close()
            .await
            .map_err(|source| archive_sqlx("close", source))?;
        if inspection.current() {
            return Ok(());
        }
    }

    let options = SqliteConnectOptions::new()
        .filename(&paths.db)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|source| archive_sqlx("open", source))?;
    let inspection = inspect_archive_schema(&mut connection).await?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        if let Err(source) = connection.close().await {
            eprintln!("archive step close failed while refusing schema: {source}");
        }
        return Err(error);
    }
    if inspection.current() {
        return connection
            .close()
            .await
            .map_err(|source| archive_sqlx("close", source));
    }
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
    let mut connection = acquire_attached(store, paths).await?;
    let batch = batch.min(ARCHIVE_BATCH_ROWS).max(1) as i64;
    let result = match copy_attached_batch(&mut connection, cutoff_ms, batch).await {
        Ok(Some(archive_batch)) => delete_attached_batch(&mut connection, archive_batch)
            .await
            .map(|deleted| (deleted, Some(archive_batch))),
        Ok(None) => Ok((0, None)),
        Err(error) => Err(error),
    };
    let (deleted, archive_batch) = detach_after(&mut connection, result).await?;
    drop(connection);

    if let Some(archive_batch) = archive_batch
        && deleted == archive_batch.row_count
    {
        let moved_until_ms = archive_batch
            .max_ms
            .saturating_add(Tier::L4.resolution_ms());
        store
            .history_state_set("archiveMovedUntilMs", &moved_until_ms, moved_until_ms)
            .await
            .map_err(|error| archive_store_error("watermark", error))?;
    }

    Ok(deleted)
}

/// Phase 1 of `move_expired_l4`, public only so the crash-order test can stop between the two commits.
#[doc(hidden)]
pub async fn copy_expired_l4_batch(
    store: &SqliteHistoryStore,
    paths: &ArchivePaths,
    cutoff_ms: i64,
    batch: usize,
) -> Result<Option<ArchiveBatch>, StoreError> {
    let mut connection = acquire_attached(store, paths).await?;
    let batch = batch.min(ARCHIVE_BATCH_ROWS).max(1) as i64;
    let result = copy_attached_batch(&mut connection, cutoff_ms, batch).await;
    detach_after(&mut connection, result).await
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

async fn acquire_attached<'a>(
    store: &'a SqliteHistoryStore,
    paths: &ArchivePaths,
) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, StoreError> {
    ensure_archive_schema(paths).await?;
    let mut connection = store
        .pool
        .acquire()
        .await
        .map_err(|source| archive_sqlx("acquire", source))?;
    let archive_db = paths.db.to_string_lossy().into_owned();
    if let Err(source) = sqlx::query("ATTACH DATABASE ?1 AS archive")
        .bind(&archive_db)
        .execute(&mut *connection)
        .await
    {
        connection.close_on_drop();
        return Err(archive_sqlx("attach", source));
    }
    Ok(connection)
}

async fn detach_after<T>(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    result: Result<T, StoreError>,
) -> Result<T, StoreError> {
    if let Err(source) = sqlx::query("DETACH DATABASE archive")
        .execute(&mut **connection)
        .await
    {
        connection.close_on_drop();
        let detach_error = archive_sqlx("detach", source);
        return match result {
            Err(operation_error) => {
                eprintln!("{detach_error}");
                Err(operation_error)
            }
            Ok(_) => Err(detach_error),
        };
    }
    result
}

async fn copy_attached_batch(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    cutoff_ms: i64,
    batch: i64,
) -> Result<Option<ArchiveBatch>, StoreError> {
    sqlx::query("BEGIN")
        .execute(&mut **connection)
        .await
        .map_err(|source| archive_sqlx("begin copy", source))?;

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
            Ok(_) => Ok(None),
            Err(source) => rollback(connection, archive_sqlx("commit copy", source)).await,
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

    // SQLite commits attached databases file-by-file in ATTACH order when `main` is WAL
    // (`aMJNeeded[WAL] = 0`, no super-journal), so a single cross-file transaction makes
    // main's DELETE durable before the archive's INSERT and a crash between them loses the
    // batch. Hence: copy + commit (archive only) -> verify the committed copy -> delete +
    // commit (main only). `OR REPLACE` keeps a stale copy from freezing; the content-matched
    // DELETE keeps a changed row until it is re-copied. ADR 0018.
    let insert_sql = format!(
        "INSERT OR REPLACE INTO archive.metric_rollups_1h ({ARCHIVE_COLUMNS}) SELECT {ARCHIVE_COLUMNS} FROM main.metric_rollups_1h WHERE bucket_start_ms BETWEEN ?1 AND ?2"
    );
    if let Err(source) = sqlx::query(sqlx::AssertSqlSafe(insert_sql))
        .bind(min_ms)
        .bind(max_ms)
        .execute(&mut **connection)
        .await
    {
        return rollback(connection, archive_sqlx("insert", source)).await;
    }

    if let Err(source) = sqlx::query("COMMIT").execute(&mut **connection).await {
        return rollback(connection, archive_sqlx("commit copy", source)).await;
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
        Err(source) => return Err(archive_sqlx("verify", source)),
    };
    if archived_count != row_count {
        let source = sqlx::Error::Protocol(format!(
            "archive batch count {archived_count} did not equal main batch count {row_count}"
        ));
        return Err(archive_sqlx("verify", source));
    }

    Ok(Some(ArchiveBatch {
        min_ms,
        max_ms,
        row_count,
    }))
}

async fn delete_attached_batch(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    batch: ArchiveBatch,
) -> Result<i64, StoreError> {
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut **connection)
        .await
        .map_err(|source| archive_sqlx("begin delete", source))?;

    let deleted = match sqlx::query(
        r#"
        DELETE FROM main.metric_rollups_1h
        WHERE bucket_start_ms BETWEEN ?1 AND ?2
          AND EXISTS (
            SELECT 1
            FROM archive.metric_rollups_1h AS a
            WHERE a.bucket_start_ms = metric_rollups_1h.bucket_start_ms
              AND a.sample_count = metric_rollups_1h.sample_count
              AND a.newest_captured_at_ms = metric_rollups_1h.newest_captured_at_ms
          )
        "#,
    )
    .bind(batch.min_ms)
    .bind(batch.max_ms)
    .execute(&mut **connection)
    .await
    {
        Ok(result) => result.rows_affected() as i64,
        Err(source) => return rollback(connection, archive_sqlx("delete", source)).await,
    };

    if let Err(source) = sqlx::query("COMMIT").execute(&mut **connection).await {
        return rollback(connection, archive_sqlx("commit delete", source)).await;
    }
    Ok(deleted)
}

async fn rollback<T>(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    error: StoreError,
) -> Result<T, StoreError> {
    if let Err(source) = sqlx::query("ROLLBACK").execute(&mut **connection).await {
        eprintln!("archive rollback failed after {error}: {source}");
    }
    Err(error)
}

fn archive_store_error(step: &'static str, error: StoreError) -> StoreError {
    match error {
        StoreError::Sqlx(source) => archive_sqlx(step, source),
        other => StoreError::Archive {
            step,
            source: ArchiveErrorSource::Io(std::io::Error::other(other.to_string())),
        },
    }
}

fn archive_sqlx(step: &'static str, source: sqlx::Error) -> StoreError {
    StoreError::Archive {
        step,
        source: ArchiveErrorSource::Sqlx(source),
    }
}
