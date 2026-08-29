use std::{
    borrow::Cow,
    fs::File,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use time::{Date, Month, OffsetDateTime, Time};

use crate::{
    ArchiveErrorSource, SqliteHistoryStore, StoreError,
    ladder::{Tier, TierBucket},
    retention_ladder::{ArchiveSettings, RetentionLadder},
    tier_bucket_from_row,
};

pub const ARCHIVE_BATCH_ROWS: usize = 1_000;
pub const MAX_ARCHIVE_BATCHES_PER_TICK: usize = 10;
pub const MAX_COLD_MONTHS_PER_PASS: usize = 12;

const ARCHIVE_SCHEMA_SQL: &str = r#"
BEGIN;

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

COMMIT;
"#;

const ARCHIVE_COLUMNS: &str = r#"
  bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
  avg_cpu_usage_percent, min_cpu_usage_percent, max_cpu_usage_percent,
  avg_memory_used_percent, min_memory_used_percent, max_memory_used_percent,
  avg_swap_used_percent, min_swap_used_percent, max_swap_used_percent,
  avg_load_percent, min_load_percent, max_load_percent,
  avg_root_used_percent, min_root_used_percent, max_root_used_percent
"#;

const DAY_MS: i64 = 86_400_000;

const ARCHIVE_ROW_MATCH: &str = r#"
  a.first_captured_at_ms = metric_rollups_1h.first_captured_at_ms
  AND a.newest_captured_at_ms = metric_rollups_1h.newest_captured_at_ms
  AND a.sample_count = metric_rollups_1h.sample_count
  AND a.avg_cpu_usage_percent = metric_rollups_1h.avg_cpu_usage_percent
  AND a.min_cpu_usage_percent = metric_rollups_1h.min_cpu_usage_percent
  AND a.max_cpu_usage_percent = metric_rollups_1h.max_cpu_usage_percent
  AND a.avg_memory_used_percent = metric_rollups_1h.avg_memory_used_percent
  AND a.min_memory_used_percent = metric_rollups_1h.min_memory_used_percent
  AND a.max_memory_used_percent = metric_rollups_1h.max_memory_used_percent
  AND a.avg_swap_used_percent = metric_rollups_1h.avg_swap_used_percent
  AND a.min_swap_used_percent = metric_rollups_1h.min_swap_used_percent
  AND a.max_swap_used_percent = metric_rollups_1h.max_swap_used_percent
  AND a.avg_load_percent = metric_rollups_1h.avg_load_percent
  AND a.min_load_percent = metric_rollups_1h.min_load_percent
  AND a.max_load_percent = metric_rollups_1h.max_load_percent
  AND a.avg_root_used_percent IS metric_rollups_1h.avg_root_used_percent
  AND a.min_root_used_percent IS metric_rollups_1h.min_root_used_percent
  AND a.max_root_used_percent IS metric_rollups_1h.max_root_used_percent
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivePaths {
    pub db: PathBuf,
    pub directory: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveManifestRow {
    pub month: String,
    pub exported_at_ms: i64,
    pub file: String,
    pub sha256: String,
    pub row_count: i64,
    pub bytes: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveSchemaState {
    Absent,
    Incomplete {
        user_version: i64,
        required_objects: i64,
    },
    Current,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveBatch {
    pub min_ms: i64,
    pub max_ms: i64,
    pub row_count: i64,
}

#[derive(Clone, Copy)]
enum ArchivePhase {
    BeforeCopyCommit,
    BetweenCommits,
    AfterDeleteCommit,
    ColdExport,
}

fn archive_phase(step: &'static str) -> ArchivePhase {
    if step.starts_with("cold ") {
        return ArchivePhase::ColdExport;
    }
    match step {
        "create directory" | "open" | "schema" | "close" | "acquire" | "attach" | "begin copy"
        | "select" | "select decode" | "insert" | "commit copy" => ArchivePhase::BeforeCopyCommit,
        "verify" | "begin delete" | "delete" | "watermark" | "commit delete" => {
            ArchivePhase::BetweenCommits
        }
        "detach" => ArchivePhase::AfterDeleteCommit,
        _ => ArchivePhase::BeforeCopyCommit,
    }
}

pub(crate) fn archive_remedy(step: &'static str) -> &'static str {
    match archive_phase(step) {
        ArchivePhase::BeforeCopyCommit => {
            "nothing was written to history-archive.sqlite and nothing was deleted from the main database; check the archive directory is writable and retry"
        }
        ArchivePhase::BetweenCommits => {
            "the batch's copy is committed in history-archive.sqlite and is refreshed on retry; nothing was deleted from the main database — retrying is safe"
        }
        ArchivePhase::AfterDeleteCommit => {
            "the batch is committed in history-archive.sqlite and removed from the main database; only the detach bookkeeping failed — retrying is safe, nothing is duplicated or lost"
        }
        ArchivePhase::ColdExport => {
            "the queryable archive is untouched; a `.tmp` file may remain in the archive directory and is safe to delete; retrying re-exports the month"
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

pub async fn archive_schema_state(paths: &ArchivePaths) -> Result<ArchiveSchemaState, StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "schema").await? else {
        return Ok(ArchiveSchemaState::Absent);
    };
    let inspection = inspect_archive_schema(&mut connection).await?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        return Err(error);
    }
    if inspection.current() {
        Ok(ArchiveSchemaState::Current)
    } else {
        Ok(ArchiveSchemaState::Incomplete {
            user_version: inspection.user_version,
            required_objects: inspection.required_object_count,
        })
    }
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
    if batch == 0 {
        return Ok(0);
    }
    let mut connection = acquire_attached(store, paths).await?;
    let batch = batch.clamp(1, ARCHIVE_BATCH_ROWS) as i64;
    let result = match copy_attached_batch(&mut connection, cutoff_ms, batch).await {
        Ok(Some(archive_batch)) => delete_attached_batch(&mut connection, archive_batch).await,
        Ok(None) => Ok(0),
        Err(error) => Err(error),
    };
    detach_after(&mut connection, result).await
}

/// Phase 1 of `move_expired_l4`, public only so the crash-order test can stop between the two commits.
#[doc(hidden)]
pub async fn copy_expired_l4_batch(
    store: &SqliteHistoryStore,
    paths: &ArchivePaths,
    cutoff_ms: i64,
    batch: usize,
) -> Result<Option<ArchiveBatch>, StoreError> {
    if batch == 0 {
        return Ok(None);
    }
    let mut connection = acquire_attached(store, paths).await?;
    let batch = batch.clamp(1, ARCHIVE_BATCH_ROWS) as i64;
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
    let inspection = inspect_archive_schema(&mut connection).await?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        return Err(error);
    }
    if !inspection.current() {
        return Ok(Vec::new());
    }
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

pub async fn read_archive_manifest(
    paths: &ArchivePaths,
) -> Result<Vec<ArchiveManifestRow>, StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "cold read").await? else {
        return Ok(Vec::new());
    };
    let inspection = inspect_archive_schema(&mut connection)
        .await
        .map_err(|error| archive_store_error("cold read", error))?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        return Err(archive_store_error("cold read", error));
    }
    if !inspection.current() {
        return Ok(Vec::new());
    }
    sqlx::query_as::<_, (String, i64, String, String, i64, i64)>(
        r#"
        SELECT month, exported_at_ms, file, sha256, row_count, bytes
        FROM archive_manifest
        ORDER BY month
        "#,
    )
    .fetch_all(&mut connection)
    .await
    .map_err(|source| archive_sqlx("cold read", source))
    .map(|rows| {
        rows.into_iter()
            .map(
                |(month, exported_at_ms, file, sha256, row_count, bytes)| ArchiveManifestRow {
                    month,
                    exported_at_ms,
                    file,
                    sha256,
                    row_count,
                    bytes,
                },
            )
            .collect()
    })
}

pub async fn archive_months_present(paths: &ArchivePaths) -> Result<Vec<String>, StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "cold months").await? else {
        return Ok(Vec::new());
    };
    let inspection = inspect_archive_schema(&mut connection)
        .await
        .map_err(|error| archive_store_error("cold months", error))?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        return Err(archive_store_error("cold months", error));
    }
    if !inspection.current() {
        return Ok(Vec::new());
    }
    read_months_on(&mut connection).await
}

pub fn exportable_months(
    months_present: &[String],
    ladder: &RetentionLadder,
    exported_until: Option<&str>,
    now_ms: i64,
) -> Vec<String> {
    if !ladder.l4.enabled || ladder.l4.keep_days == 0 {
        return Vec::new();
    }
    let Some(age_cutoff) = calendar_month_before(now_ms, ladder.archive.cold_after_months) else {
        return Vec::new();
    };
    let l4_keep_ms = ladder.l4.keep_days.saturating_mul(DAY_MS);
    let mut months = months_present
        .iter()
        .filter(|month| month.as_str() <= age_cutoff.as_str())
        .filter(|month| exported_until.is_none_or(|watermark| month.as_str() > watermark))
        .filter(|month| {
            month_bounds(month).is_some_and(|(_, next_start_ms)| {
                next_start_ms
                    .saturating_sub(1)
                    .saturating_add(l4_keep_ms)
                    .saturating_add(DAY_MS)
                    <= now_ms
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    months.sort();
    months.dedup();
    months
}

pub async fn months_ready_to_export(
    store: &SqliteHistoryStore,
    months: &[String],
) -> Result<Vec<String>, StoreError> {
    let mut ready = Vec::with_capacity(months.len());
    for month in months {
        let (start_ms, next_start_ms) = month_bounds(month).ok_or_else(|| StoreError::Archive {
            step: "cold months",
            source: ArchiveErrorSource::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("archive month {month:?} is not UTC YYYY-MM"),
            )),
        })?;
        let main_rows: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM metric_rollups_1h
            WHERE bucket_start_ms >= ?1 AND bucket_start_ms < ?2
            "#,
        )
        .bind(start_ms)
        .bind(next_start_ms)
        .fetch_one(&store.pool)
        .await
        .map_err(|source| archive_sqlx("cold months", source))?;
        if main_rows != 0 {
            break;
        }
        ready.push(month.clone());
    }
    Ok(ready)
}

pub async fn export_cold_months(
    store: &SqliteHistoryStore,
    paths: &ArchivePaths,
    ladder: &RetentionLadder,
    now_ms: i64,
) -> Result<Vec<ArchiveManifestRow>, StoreError> {
    if !paths
        .db
        .try_exists()
        .map_err(|source| StoreError::Archive {
            step: "cold read",
            source: ArchiveErrorSource::Io(source),
        })?
    {
        return Ok(Vec::new());
    }
    let options = SqliteConnectOptions::new()
        .filename(&paths.db)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|source| archive_sqlx("cold read", source))?;
    let inspection = inspect_archive_schema(&mut connection)
        .await
        .map_err(|error| archive_store_error("cold read", error))?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        return Err(archive_store_error("cold read", error));
    }
    if !inspection.current() {
        return Err(StoreError::Archive {
            step: "cold read",
            source: ArchiveErrorSource::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "history-archive.sqlite at {} is incomplete; expected metric_rollups_1h, its newest index, archive_manifest, and user_version 1",
                    paths.db.display()
                ),
            )),
        });
    }
    let months_present = read_months_on(&mut connection).await?;
    let exported_until = store
        .history_state_get::<String>("coldExportedUntilMonth")
        .await
        .map_err(|error| archive_store_error("cold watermark", error))?;
    let candidates = exportable_months(&months_present, ladder, exported_until.as_deref(), now_ms);
    let candidate_count = candidates.len().min(MAX_COLD_MONTHS_PER_PASS);
    let months = months_ready_to_export(store, &candidates[..candidate_count]).await?;
    let mut written = Vec::with_capacity(months.len());
    for month in months {
        let (start_ms, next_start_ms) =
            month_bounds(&month).ok_or_else(|| StoreError::Archive {
                step: "cold months",
                source: ArchiveErrorSource::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("archive month {month:?} is not UTC YYYY-MM"),
                )),
            })?;
        let sql = format!(
            "SELECT {ARCHIVE_COLUMNS} FROM metric_rollups_1h WHERE bucket_start_ms >= ?1 AND bucket_start_ms < ?2 ORDER BY bucket_start_ms"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(start_ms)
            .bind(next_start_ms)
            .fetch_all(&mut connection)
            .await
            .map_err(|source| archive_sqlx("cold read", source))?;
        if rows.is_empty() {
            continue;
        }
        let expected_first_ms = rows[0]
            .try_get("bucket_start_ms")
            .map_err(|source| archive_sqlx("cold read", source))?;
        let expected_last_ms = rows[rows.len() - 1]
            .try_get("bucket_start_ms")
            .map_err(|source| archive_sqlx("cold read", source))?;
        let row_count = i64::try_from(rows.len()).map_err(|_| StoreError::Archive {
            step: "cold read",
            source: ArchiveErrorSource::Io(std::io::Error::other(
                "cold month row count does not fit SQLite INTEGER",
            )),
        })?;
        let file_name = format!("tinytop-1h-{month}.csv.gz");
        let target = paths.directory.join(&file_name);
        let temporary = suffixed_path(&target, ".tmp");
        write_cold_file(&temporary, &rows)?;
        let sha256 = sha256_file(&temporary)?;
        verify_cold_file(&temporary, row_count, expected_first_ms, expected_last_ms)?;
        std::fs::rename(&temporary, &target).map_err(|source| StoreError::Archive {
            step: "cold rename",
            source: ArchiveErrorSource::Io(source),
        })?;
        sync_directory(&paths.directory)?;
        let sidecar = suffixed_path(&target, ".sha256");
        let mut sidecar_file = File::create(&sidecar).map_err(|source| StoreError::Archive {
            step: "cold sidecar",
            source: ArchiveErrorSource::Io(source),
        })?;
        sidecar_file
            .write_all(format!("{sha256}  {file_name}\n").as_bytes())
            .and_then(|()| sidecar_file.sync_all())
            .map_err(|source| StoreError::Archive {
                step: "cold sidecar",
                source: ArchiveErrorSource::Io(source),
            })?;
        let bytes = i64::try_from(
            std::fs::metadata(&target)
                .map_err(|source| StoreError::Archive {
                    step: "cold manifest",
                    source: ArchiveErrorSource::Io(source),
                })?
                .len(),
        )
        .map_err(|_| StoreError::Archive {
            step: "cold manifest",
            source: ArchiveErrorSource::Io(std::io::Error::other(
                "cold file byte count does not fit SQLite INTEGER",
            )),
        })?;
        let manifest = ArchiveManifestRow {
            month: month.clone(),
            exported_at_ms: now_ms,
            file: file_name,
            sha256,
            row_count,
            bytes,
        };
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO archive_manifest
              (month, exported_at_ms, file, sha256, row_count, bytes)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(&manifest.month)
        .bind(manifest.exported_at_ms)
        .bind(&manifest.file)
        .bind(&manifest.sha256)
        .bind(manifest.row_count)
        .bind(manifest.bytes)
        .execute(&mut connection)
        .await
        .map_err(|source| archive_sqlx("cold manifest", source))?;
        store
            .history_state_set("coldExportedUntilMonth", &month, now_ms)
            .await
            .map_err(|error| archive_store_error("cold watermark", error))?;
        written.push(manifest);
    }
    Ok(written)
}

pub(crate) fn csv_field(raw: &str) -> Cow<'_, str> {
    if raw
        .as_bytes()
        .iter()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
    {
        Cow::Owned(format!("\"{}\"", raw.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(raw)
    }
}

#[doc(hidden)]
pub fn verify_cold_file(
    path: &Path,
    expected_rows: i64,
    expected_first_ms: i64,
    expected_last_ms: i64,
) -> Result<(), StoreError> {
    let file = File::open(path).map_err(cold_verify_io)?;
    let mut decoder = GzDecoder::new(file);
    let mut csv = String::new();
    decoder.read_to_string(&mut csv).map_err(cold_verify_io)?;
    let records = parse_rfc4180(&csv).map_err(cold_verify_io)?;
    let expected_header = archive_column_names().join(",");
    let header = records.first().ok_or_else(|| {
        cold_verify_io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cold CSV has no header",
        ))
    })?;
    if header.join(",") != expected_header {
        return Err(cold_verify_io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cold CSV header does not match archive DDL column order",
        )));
    }
    let data = &records[1..];
    let header_width = header.len();
    for (index, record) in data.iter().enumerate() {
        if record.len() != header_width {
            return Err(cold_verify_io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "cold CSV data record {} has {} fields; expected {header_width} from header",
                    index + 1,
                    record.len()
                ),
            )));
        }
    }
    let observed_rows = i64::try_from(data.len()).unwrap_or(i64::MAX);
    if observed_rows != expected_rows {
        return Err(cold_verify_io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("cold CSV row count {observed_rows} did not equal expected {expected_rows}"),
        )));
    }
    let first_ms = parse_first_bucket(data.first(), "first")?;
    let last_ms = parse_first_bucket(data.last(), "last")?;
    if first_ms != expected_first_ms || last_ms != expected_last_ms {
        return Err(cold_verify_io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "cold CSV first/last bucket {first_ms}/{last_ms} did not equal expected {expected_first_ms}/{expected_last_ms}"
            ),
        )));
    }
    Ok(())
}

async fn read_months_on(connection: &mut SqliteConnection) -> Result<Vec<String>, StoreError> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT strftime('%Y-%m', bucket_start_ms / 1000.0, 'unixepoch')
        FROM metric_rollups_1h
        WHERE strftime('%Y-%m', bucket_start_ms / 1000.0, 'unixepoch') IS NOT NULL
        ORDER BY 1
        "#,
    )
    .fetch_all(connection)
    .await
    .map_err(|source| archive_sqlx("cold months", source))
}

fn calendar_month_before(now_ms: i64, months: i64) -> Option<String> {
    let now = OffsetDateTime::from_unix_timestamp(now_ms.div_euclid(1_000)).ok()?;
    let month_index = now
        .year()
        .checked_mul(12)?
        .checked_add(now.month() as i32 - 1)?
        .checked_sub(i32::try_from(months).ok()?)?;
    let year = month_index.div_euclid(12);
    let month = month_index.rem_euclid(12) + 1;
    Some(format!("{year:04}-{month:02}"))
}

fn month_bounds(month: &str) -> Option<(i64, i64)> {
    let (year, month_number) = month.split_once('-')?;
    if year.len() != 4 || month_number.len() != 2 {
        return None;
    }
    let year = year.parse::<i32>().ok()?;
    let month_number = month_number.parse::<u8>().ok()?;
    let month = Month::try_from(month_number).ok()?;
    let start = utc_date_ms(year, month)?;
    let (next_year, next_month) = if month == Month::December {
        (year.checked_add(1)?, Month::January)
    } else {
        (year, Month::try_from(month_number.checked_add(1)?).ok()?)
    };
    Some((start, utc_date_ms(next_year, next_month)?))
}

fn utc_date_ms(year: i32, month: Month) -> Option<i64> {
    let date = Date::from_calendar_date(year, month, 1).ok()?;
    let nanos = date
        .with_time(Time::MIDNIGHT)
        .assume_utc()
        .unix_timestamp_nanos();
    i64::try_from(nanos.div_euclid(1_000_000)).ok()
}

fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn archive_column_names() -> Vec<&'static str> {
    ARCHIVE_COLUMNS.split(',').map(str::trim).collect()
}

fn write_cold_file(path: &Path, rows: &[sqlx::sqlite::SqliteRow]) -> Result<(), StoreError> {
    let file = File::create(path).map_err(|source| StoreError::Archive {
        step: "cold write",
        source: ArchiveErrorSource::Io(source),
    })?;
    let mut encoder = GzEncoder::new(file, Compression::new(6));
    let column_names = archive_column_names();
    write_csv_record(&mut encoder, column_names.iter().copied()).map_err(|source| {
        StoreError::Archive {
            step: "cold write",
            source: ArchiveErrorSource::Io(source),
        }
    })?;
    for row in rows {
        let fields = archive_csv_fields(row)?;
        write_csv_record(&mut encoder, fields.iter().map(String::as_str)).map_err(|source| {
            StoreError::Archive {
                step: "cold write",
                source: ArchiveErrorSource::Io(source),
            }
        })?;
    }
    let file = encoder.finish().map_err(|source| StoreError::Archive {
        step: "cold write",
        source: ArchiveErrorSource::Io(source),
    })?;
    file.sync_all().map_err(|source| StoreError::Archive {
        step: "cold fsync",
        source: ArchiveErrorSource::Io(source),
    })
}

fn archive_csv_fields(row: &sqlx::sqlite::SqliteRow) -> Result<Vec<String>, StoreError> {
    let column_names = archive_column_names();
    let mut fields = Vec::with_capacity(column_names.len());
    for column in &column_names[..4] {
        let value: i64 = row
            .try_get(*column)
            .map_err(|source| archive_sqlx("cold read", source))?;
        fields.push(value.to_string());
    }
    for column in &column_names[4..16] {
        let value: f64 = row
            .try_get(*column)
            .map_err(|source| archive_sqlx("cold read", source))?;
        fields.push(value.to_string());
    }
    for column in &column_names[16..] {
        let value: Option<f64> = row
            .try_get(*column)
            .map_err(|source| archive_sqlx("cold read", source))?;
        fields.push(value.map_or_else(String::new, |value| value.to_string()));
    }
    Ok(fields)
}

fn write_csv_record<'a>(
    writer: &mut impl Write,
    fields: impl IntoIterator<Item = &'a str>,
) -> std::io::Result<()> {
    let mut first = true;
    for field in fields {
        if !first {
            writer.write_all(b",")?;
        }
        first = false;
        writer.write_all(csv_field(field).as_bytes())?;
    }
    writer.write_all(b"\r\n")
}

fn sha256_file(path: &Path) -> Result<String, StoreError> {
    let file = File::open(path).map_err(|source| StoreError::Archive {
        step: "cold hash",
        source: ArchiveErrorSource::Io(source),
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| StoreError::Archive {
                step: "cold hash",
                source: ArchiveErrorSource::Io(source),
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(hex)
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), StoreError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|source| StoreError::Archive {
            step: "cold directory fsync",
            source: ArchiveErrorSource::Io(source),
        })
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), StoreError> {
    Ok(())
}

fn parse_rfc4180(csv: &str) -> std::io::Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = csv.chars().peekable();
    let mut quoted = false;
    let mut after_closing_quote = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    after_closing_quote = true;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        let record_number = rows.len() + 1;
        if after_closing_quote && !matches!(ch, ',' | '\r') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "cold CSV record {record_number} has character {ch:?} after a closing quote"
                ),
            ));
        }
        match ch {
            '"' if field.is_empty() => quoted = true,
            '"' => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("cold CSV record {record_number} has a quote inside an unquoted field"),
                ));
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                after_closing_quote = false;
            }
            '\r' => {
                if chars.next() != Some('\n') {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "cold CSV record has CR without LF",
                    ));
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                after_closing_quote = false;
            }
            '\n' => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "cold CSV record has bare LF",
                ));
            }
            _ => field.push(ch),
        }
    }
    if quoted || !field.is_empty() || !row.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cold CSV final record is incomplete",
        ));
    }
    Ok(rows)
}

fn parse_first_bucket(record: Option<&Vec<String>>, position: &str) -> Result<i64, StoreError> {
    record
        .and_then(|record| record.first())
        .ok_or_else(|| {
            cold_verify_io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("cold CSV has no {position} data record"),
            ))
        })?
        .parse::<i64>()
        .map_err(|error| {
            cold_verify_io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("cold CSV {position} bucket is not an integer: {error}"),
            ))
        })
}

fn cold_verify_io(source: std::io::Error) -> StoreError {
    StoreError::Archive {
        step: "cold verify",
        source: ArchiveErrorSource::Io(source),
    }
}

pub(super) async fn archive_coverage(
    paths: &ArchivePaths,
) -> Result<(i64, Option<i64>, Option<i64>), StoreError> {
    let Some(mut connection) = open_archive_read_only(paths, "coverage open").await? else {
        return Ok((0, None, None));
    };
    let inspection = inspect_archive_schema(&mut connection)
        .await
        .map_err(|error| archive_store_error("coverage", error))?;
    if let Some(error) = archive_schema_refusal(paths, inspection) {
        return Err(archive_store_error("coverage", error));
    }
    if !inspection.current() {
        return Ok((0, None, None));
    }
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

async fn acquire_attached(
    store: &SqliteHistoryStore,
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
    if let Err(source) = sqlx::query("PRAGMA archive.synchronous = FULL")
        .execute(&mut *connection)
        .await
    {
        connection.close_on_drop();
        return Err(archive_sqlx("attach", source));
    }
    let synchronous: i64 = match sqlx::query_scalar("PRAGMA archive.synchronous")
        .fetch_one(&mut *connection)
        .await
    {
        Ok(value) => value,
        Err(source) => {
            connection.close_on_drop();
            return Err(archive_sqlx("attach", source));
        }
    };
    if synchronous != 2 {
        connection.close_on_drop();
        return Err(StoreError::Archive {
            step: "attach",
            source: ArchiveErrorSource::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("archive synchronous is {synchronous}, expected 2 (FULL)"),
            )),
        });
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
    // commit (main only). `OR REPLACE` keeps a stale copy from freezing; key-set verification
    // ignores unrelated archive keys in the selected interval, and full-row equality keeps a
    // changed row until it is re-copied. The archive commit is fsynced before phase B, whose
    // watermark upsert commits with the delete. ADRs 0018 and 0019.
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
        r#"
        SELECT COUNT(*)
        FROM main.metric_rollups_1h AS m
        WHERE m.bucket_start_ms BETWEEN ?1 AND ?2
          AND EXISTS (
            SELECT 1
            FROM archive.metric_rollups_1h AS a
            WHERE a.bucket_start_ms = m.bucket_start_ms
          )
        "#,
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

    let delete_sql = format!(
        r#"
        DELETE FROM main.metric_rollups_1h
        WHERE bucket_start_ms BETWEEN ?1 AND ?2
          AND EXISTS (
            SELECT 1
            FROM archive.metric_rollups_1h AS a
            WHERE a.bucket_start_ms = metric_rollups_1h.bucket_start_ms
              AND {ARCHIVE_ROW_MATCH}
          )
        "#
    );
    let deleted = match sqlx::query(sqlx::AssertSqlSafe(delete_sql))
        .bind(batch.min_ms)
        .bind(batch.max_ms)
        .execute(&mut **connection)
        .await
    {
        Ok(result) => result.rows_affected() as i64,
        Err(source) => return rollback(connection, archive_sqlx("delete", source)).await,
    };

    if deleted == batch.row_count {
        let moved_until_ms = batch.max_ms.saturating_add(Tier::L4.resolution_ms());
        if let Err(error) = crate::history_state_set_on(
            &mut *connection,
            "archiveMovedUntilMs",
            &moved_until_ms,
            crate::now_ms(),
        )
        .await
        {
            return rollback(connection, archive_store_error("watermark", error)).await;
        }
    }

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
        StoreError::Archive { source, .. } => StoreError::Archive { step, source },
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

#[cfg(test)]
mod cold_export_tests {
    use super::{csv_field, exportable_months};
    use crate::retention_ladder::RetentionLadder;

    const AUG_29_2026_MS: i64 = 1_787_961_600_000;

    #[test]
    fn exportable_months_apply_age_expiry_and_watermark_rules() {
        // Break caught: age alone exports a month while L4 can still feed later hours into it.
        struct Case {
            name: &'static str,
            configure: fn(&mut RetentionLadder),
            exported_until: Option<&'static str>,
            months: &'static [&'static str],
            expected: &'static [&'static str],
        }

        fn defaults(_: &mut RetentionLadder) {}
        fn one_month(ladder: &mut RetentionLadder) {
            ladder.archive.cold_after_months = 1;
            ladder.l4.keep_days = 30;
        }
        fn forever(ladder: &mut RetentionLadder) {
            ladder.l4.keep_days = 0;
        }
        fn disabled(ladder: &mut RetentionLadder) {
            ladder.l4.enabled = false;
        }

        let cases = [
            Case {
                name: "defaults",
                configure: defaults,
                exported_until: None,
                months: &["2024-01", "2024-02", "2026-07"],
                expected: &["2024-01", "2024-02"],
            },
            Case {
                name: "cold after one month",
                configure: one_month,
                exported_until: None,
                months: &["2026-05", "2026-06"],
                expected: &["2026-05", "2026-06"],
            },
            Case {
                name: "L4 forever",
                configure: forever,
                exported_until: None,
                months: &["2023-01"],
                expected: &[],
            },
            Case {
                name: "L4 disabled",
                configure: disabled,
                exported_until: None,
                months: &["2023-01"],
                expected: &[],
            },
            Case {
                name: "month partially expired",
                configure: one_month,
                exported_until: None,
                months: &["2026-07"],
                expected: &[],
            },
            Case {
                name: "watermark equal",
                configure: one_month,
                exported_until: Some("2026-05"),
                months: &["2026-05", "2026-06"],
                expected: &["2026-06"],
            },
            Case {
                name: "watermark greater",
                configure: one_month,
                exported_until: Some("2026-07"),
                months: &["2026-05", "2026-06"],
                expected: &[],
            },
        ];

        for case in cases {
            let mut ladder = RetentionLadder::default();
            (case.configure)(&mut ladder);
            let present = case
                .months
                .iter()
                .map(|month| (*month).to_string())
                .collect::<Vec<_>>();
            assert_eq!(
                exportable_months(&present, &ladder, case.exported_until, AUG_29_2026_MS),
                case.expected,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn csv_field_quotes_rfc4180_special_characters() {
        // Break caught: commas, quotes, CR, or LF escape the field boundary.
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(
            csv_field("comma, quote\" and\r\nline"),
            "\"comma, quote\"\" and\r\nline\""
        );
    }
}
