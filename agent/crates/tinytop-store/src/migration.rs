use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::{StoreError, disk};

pub const SCHEMA_VERSION: i64 = 2;
pub(crate) const DEFAULT_SNAPSHOT_JSON_KEEP_MS: i64 = 60 * 60 * 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub from: i64,
    pub to: i64,
    pub pre_image_path: Option<PathBuf>,
    pub samples_kept: i64,
    pub json_rows_kept: i64,
    pub bytes_before: i64,
    pub vacuumed_at_ms: Option<i64>,
    pub bytes_after: Option<i64>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationAudit {
    #[serde(flatten)]
    report: MigrationReport,
    started_at_ms: i64,
}

#[doc(hidden)]
pub const CREATE_SCHEMA_V1_SQL: &str = r#"
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
  snapshot_json TEXT
);

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
  avg_root_used_percent REAL,
  min_cpu_usage_percent REAL,
  min_memory_used_percent REAL,
  min_swap_used_percent REAL,
  min_load_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1m_newest
  ON metric_rollups_1m (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS metric_rollups_5m (
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

CREATE INDEX IF NOT EXISTS idx_metric_rollups_5m_newest
  ON metric_rollups_5m (newest_captured_at_ms DESC);

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

CREATE TABLE IF NOT EXISTS history_state (
  state_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS fs_samples (
  captured_at_ms INTEGER NOT NULL,
  mount TEXT NOT NULL,
  filesystem TEXT NOT NULL,
  fs_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  used_bytes INTEGER NOT NULL,
  available_bytes INTEGER NOT NULL,
  used_percent REAL NOT NULL,
  inode_used_percent REAL,
  inode_used INTEGER,
  inode_total INTEGER,
  PRIMARY KEY (captured_at_ms, mount)
);

CREATE INDEX IF NOT EXISTS idx_fs_samples_mount_time
  ON fs_samples (mount, captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS process_samples (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command TEXT NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at TEXT,
  PRIMARY KEY (captured_at_ms, rank)
);

CREATE INDEX IF NOT EXISTS idx_process_samples_time
  ON process_samples (captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);

PRAGMA user_version = 1;
"#;

const CREATE_SCHEMA_V2_HEAD_SQL: &str = r#"
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
  snapshot_json TEXT
);

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
  avg_root_used_percent REAL,
  min_cpu_usage_percent REAL,
  min_memory_used_percent REAL,
  min_swap_used_percent REAL,
  min_load_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1m_newest
  ON metric_rollups_1m (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS metric_rollups_5m (
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

CREATE INDEX IF NOT EXISTS idx_metric_rollups_5m_newest
  ON metric_rollups_5m (newest_captured_at_ms DESC);

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

CREATE TABLE IF NOT EXISTS history_state (
  state_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS fs_samples (
  captured_at_ms INTEGER NOT NULL,
  mount TEXT NOT NULL,
  filesystem TEXT NOT NULL,
  fs_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  used_bytes INTEGER NOT NULL,
  available_bytes INTEGER NOT NULL,
  used_percent REAL NOT NULL,
  inode_used_percent REAL,
  inode_used INTEGER,
  inode_total INTEGER,
  PRIMARY KEY (captured_at_ms, mount)
);

CREATE INDEX IF NOT EXISTS idx_fs_samples_mount_time
  ON fs_samples (mount, captured_at_ms DESC);
"#;

const CREATE_PROCESS_COMMANDS_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS process_commands (
  command_id INTEGER PRIMARY KEY,
  command TEXT NOT NULL UNIQUE
);
"#;

const CREATE_PROCESS_SAMPLES_FAST_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS process_samples_fast (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command_id INTEGER NOT NULL REFERENCES process_commands(command_id),
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at TEXT,
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_process_samples_fast_command
  ON process_samples_fast (command_id);
"#;

const CREATE_PROCESS_SAMPLES_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS process_samples (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at TEXT,
  command_id INTEGER REFERENCES process_commands(command_id),
  PRIMARY KEY (captured_at_ms, rank)
);

CREATE INDEX IF NOT EXISTS idx_process_samples_time
  ON process_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_process_samples_command
  ON process_samples (command_id);
"#;

const CREATE_SCHEMA_V2_TAIL_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);

PRAGMA user_version = 2;
"#;

const CREATE_SCHEMA_V2_SQL: [&str; 5] = [
    CREATE_SCHEMA_V2_HEAD_SQL,
    CREATE_PROCESS_COMMANDS_V2_SQL,
    CREATE_PROCESS_SAMPLES_FAST_V2_SQL,
    CREATE_PROCESS_SAMPLES_V2_SQL,
    CREATE_SCHEMA_V2_TAIL_SQL,
];

const CREATE_METRIC_SAMPLES_V1_TEMP_SQL: &str = r#"
CREATE TABLE metric_samples_v1 (
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
  snapshot_json TEXT
)
"#;

const COPY_METRIC_SAMPLES_TO_V1_SQL: &str = r#"
INSERT INTO metric_samples_v1 (
  sample_id,
  captured_at_ms,
  snapshot_timestamp,
  hostname,
  runtime_kind,
  cpu_usage_percent,
  cpu_cores,
  memory_used_percent,
  memory_used_bytes,
  memory_total_bytes,
  swap_used_percent,
  swap_used_bytes,
  swap_total_bytes,
  load_one,
  load_five,
  load_fifteen,
  load_percent,
  runnable_threads,
  total_threads,
  root_used_percent,
  snapshot_json
)
SELECT
  sample_id,
  captured_at_ms,
  snapshot_timestamp,
  hostname,
  runtime_kind,
  cpu_usage_percent,
  cpu_cores,
  memory_used_percent,
  memory_used_bytes,
  memory_total_bytes,
  swap_used_percent,
  swap_used_bytes,
  swap_total_bytes,
  load_one,
  load_five,
  load_fifteen,
  load_percent,
  runnable_threads,
  total_threads,
  root_used_percent,
  CASE WHEN captured_at_ms >= ? THEN snapshot_json ELSE NULL END
FROM metric_samples
"#;

const ROLLUP_1M_ADDITIVE_COLUMNS: [(&str, &str); 6] = [
    (
        "min_cpu_usage_percent",
        "ALTER TABLE metric_rollups_1m ADD COLUMN min_cpu_usage_percent REAL",
    ),
    (
        "min_memory_used_percent",
        "ALTER TABLE metric_rollups_1m ADD COLUMN min_memory_used_percent REAL",
    ),
    (
        "min_swap_used_percent",
        "ALTER TABLE metric_rollups_1m ADD COLUMN min_swap_used_percent REAL",
    ),
    (
        "min_load_percent",
        "ALTER TABLE metric_rollups_1m ADD COLUMN min_load_percent REAL",
    ),
    (
        "min_root_used_percent",
        "ALTER TABLE metric_rollups_1m ADD COLUMN min_root_used_percent REAL",
    ),
    (
        "max_root_used_percent",
        "ALTER TABLE metric_rollups_1m ADD COLUMN max_root_used_percent REAL",
    ),
];

pub(crate) async fn ensure_schema(
    pool: &SqlitePool,
    db_path: &Path,
    now_ms: i64,
    snapshot_json_keep_ms: i64,
) -> Result<Option<MigrationReport>, StoreError> {
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;

    match user_version {
        SCHEMA_VERSION => {
            apply_schema_v2(pool).await?;
            complete_pending_migration(pool, db_path).await
        }
        1 => {
            let report = complete_pending_migration(pool, db_path).await?;
            migrate_v1_to_v2(pool, db_path, now_ms).await?;
            Ok(report)
        }
        0 => {
            let metric_samples_exists = table_exists(pool, "metric_samples").await?;
            if !metric_samples_exists {
                apply_schema_v2(pool).await?;
                return Ok(None);
            }

            let sample_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples")
                .fetch_one(pool)
                .await?;
            if sample_count == 0 {
                rebuild_v0_schema(pool, now_ms, snapshot_json_keep_ms, None).await?;
                migrate_v1_to_v2(pool, db_path, now_ms).await?;
                return Ok(None);
            }

            let report =
                migrate_populated_v0(pool, db_path, now_ms, snapshot_json_keep_ms, sample_count)
                    .await?;
            migrate_v1_to_v2(pool, db_path, now_ms).await?;
            Ok(Some(report))
        }
        other => Err(StoreError::Migration {
            reason: format!(
                "unsupported SQLite schema version {other} at {} (supported version is {SCHEMA_VERSION})",
                db_path.display()
            ),
            remedy:
                "upgrade tinytop-agent to a version that supports this database before retrying"
                    .to_string(),
        }),
    }
}

async fn apply_schema_v2(pool: &SqlitePool) -> Result<(), StoreError> {
    apply_schema_groups(pool, &CREATE_SCHEMA_V2_SQL).await
}

async fn apply_schema_groups(
    pool: &SqlitePool,
    statement_groups: &[&'static str],
) -> Result<(), StoreError> {
    let mut transaction = pool.begin().await?;
    for statement_group in statement_groups {
        sqlx::raw_sql(*statement_group)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(())
}

async fn migrate_v1_to_v2(
    pool: &SqlitePool,
    _db_path: &Path,
    now_ms: i64,
) -> Result<(), StoreError> {
    let linked_version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(pool)
        .await?;
    require_sqlite_at_least(&linked_version, (3, 35, 0))?;

    let started = Instant::now();
    let mut transaction = pool.begin().await?;
    sqlx::raw_sql(CREATE_PROCESS_COMMANDS_V2_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::raw_sql(CREATE_PROCESS_SAMPLES_FAST_V2_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "ALTER TABLE process_samples ADD COLUMN command_id INTEGER REFERENCES process_commands(command_id)",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO process_commands (command) SELECT DISTINCT command FROM process_samples",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE process_samples SET command_id = (SELECT command_id FROM process_commands WHERE command = process_samples.command)",
    )
    .execute(&mut *transaction)
    .await?;

    let missing_command_ids: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM process_samples WHERE command_id IS NULL")
            .fetch_one(&mut *transaction)
            .await?;
    if missing_command_ids > 0 {
        return Err(StoreError::Migration {
            reason: format!(
                "schema v2 backfill left {missing_command_ids} process_samples rows without a command_id"
            ),
            remedy:
                "report this with `tinytop-agent db stats --json`; the database was not modified"
                    .to_string(),
        });
    }

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_process_samples_command ON process_samples (command_id)",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query("ALTER TABLE process_samples DROP COLUMN command")
        .execute(&mut *transaction)
        .await
        .map_err(|error| StoreError::Migration {
            reason: format!(
                "ALTER TABLE process_samples DROP COLUMN command failed inside the v1→v2 transaction: {error} (linked SQLite {linked_version})"
            ),
            remedy: "remove the index, trigger or view that references process_samples.command and restart; the database was not modified".to_string(),
        })?;

    let commands_interned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM process_commands")
        .fetch_one(&mut *transaction)
        .await?;
    let process_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM process_samples")
        .fetch_one(&mut *transaction)
        .await?;
    let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let details_json = serde_json::to_string(&serde_json::json!({
        "fromVersion": 1,
        "toVersion": 2,
        "commandsInterned": commands_interned,
        "processRows": process_rows,
        "durationMs": duration_ms,
    }))?;
    sqlx::query(
        r#"
        INSERT INTO app_events (occurred_at_ms, marker_type, label, details_json)
        VALUES (?, 'schemaMigrated', 'SQLite schema migrated from v1 to v2', ?)
        "#,
    )
    .bind(now_ms)
    .bind(details_json)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("PRAGMA user_version = 2")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    eprintln!(
        "history migration info: schema v1 → v2 in {duration_ms} ms ({commands_interned} commands interned over {process_rows} process rows)"
    );
    Ok(())
}

#[doc(hidden)]
pub fn require_sqlite_at_least(version: &str, minimum: (u64, u64, u64)) -> Result<(), StoreError> {
    let parsed = version
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok();
    let meets_minimum = parsed
        .as_deref()
        .and_then(|parts| match parts {
            [major, minor, patch] => Some((*major, *minor, *patch) >= minimum),
            _ => None,
        })
        .unwrap_or(false);
    if meets_minimum {
        Ok(())
    } else {
        Err(sqlite_version_refusal(version))
    }
}

fn sqlite_version_refusal(version: &str) -> StoreError {
    StoreError::Migration {
        reason: format!("schema migration requires SQLite ≥ 3.35.0 (linked: {version})"),
        remedy:
            "rebuild tinytop-agent against a bundled SQLite 3.35.0 or newer; no migration was attempted"
                .to_string(),
    }
}

async fn migrate_populated_v0(
    pool: &SqlitePool,
    db_path: &Path,
    now_ms: i64,
    snapshot_json_keep_ms: i64,
    sample_count: i64,
) -> Result<MigrationReport, StoreError> {
    let (canonical_db_path, audit) = migrate_populated_v0_schema_phase_inner(
        pool,
        db_path,
        now_ms,
        snapshot_json_keep_ms,
        sample_count,
    )
    .await?;
    Ok(
        vacuum_and_complete_migration(pool, &canonical_db_path, audit)
            .await?
            .report,
    )
}

/// Test seam for an authentic crash after the v1 schema transaction commits
/// and before the post-commit VACUUM/audit-marker phase begins.
#[doc(hidden)]
pub async fn migrate_populated_v0_schema_phase(
    pool: &SqlitePool,
    db_path: &Path,
    now_ms: i64,
    snapshot_json_keep_ms: i64,
) -> Result<MigrationReport, StoreError> {
    let sample_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples")
        .fetch_one(pool)
        .await?;
    let (_, audit) = migrate_populated_v0_schema_phase_inner(
        pool,
        db_path,
        now_ms,
        snapshot_json_keep_ms,
        sample_count,
    )
    .await?;
    Ok(audit.report.clone())
}

async fn migrate_populated_v0_schema_phase_inner(
    pool: &SqlitePool,
    db_path: &Path,
    now_ms: i64,
    snapshot_json_keep_ms: i64,
    sample_count: i64,
) -> Result<(PathBuf, MigrationAudit), StoreError> {
    let canonical_db_path = canonical_database_path(db_path)?;
    let pre_image_path = pre_image_path(&canonical_db_path);
    refuse_existing_pre_image(&pre_image_path)?;

    let database_bytes = database_bytes_with_wal(&canonical_db_path)?;
    let required_bytes = disk::required_pre_image_bytes(database_bytes);
    let database_dir = canonical_db_path
        .parent()
        .ok_or_else(|| StoreError::Migration {
            reason: format!(
                "database path {} has no parent directory",
                canonical_db_path.display()
            ),
            remedy: "move the database to an absolute path with a parent directory and retry"
                .to_string(),
        })?;
    let free_bytes = disk::free_bytes_at(database_dir).map_err(|error| StoreError::Migration {
        reason: undeterminable_free_space_reason(
            &canonical_db_path,
            database_bytes,
            required_bytes,
            &error,
        ),
        remedy: format!(
            "make the database filesystem visible and ensure at least {required_bytes} bytes are free, then retry"
        ),
    })?;
    if !disk::has_pre_image_headroom(database_bytes, free_bytes) {
        let bytes_to_free = required_bytes.saturating_sub(free_bytes);
        return Err(StoreError::Migration {
            reason: format!(
                "database {} has {free_bytes} free bytes but needs {required_bytes} bytes for a pre-image of {database_bytes} bytes",
                canonical_db_path.display()
            ),
            remedy: format!(
                "free at least {bytes_to_free} bytes on {} and retry; no migration was attempted",
                database_dir.display()
            ),
        });
    }

    create_pre_image(pool, &pre_image_path).await?;

    let cutoff_ms = now_ms.saturating_sub(snapshot_json_keep_ms);
    let json_rows_kept: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM metric_samples WHERE captured_at_ms >= ? AND snapshot_json IS NOT NULL",
    )
    .bind(cutoff_ms)
    .fetch_one(pool)
    .await?;
    let audit = MigrationAudit {
        report: MigrationReport {
            from: 0,
            to: 1,
            pre_image_path: Some(pre_image_path),
            samples_kept: sample_count,
            json_rows_kept,
            bytes_before: bytes_to_i64(database_bytes, "pre-migration database")?,
            vacuumed_at_ms: None,
            bytes_after: None,
            duration_ms: None,
        },
        started_at_ms: now_ms,
    };
    rebuild_v0_schema(pool, now_ms, snapshot_json_keep_ms, Some(&audit)).await?;
    Ok((canonical_db_path, audit))
}

fn undeterminable_free_space_reason(
    db_path: &Path,
    database_bytes: u64,
    required_bytes: u64,
    error: &io::Error,
) -> String {
    format!(
        "cannot determine free bytes for database {} with {database_bytes} bytes; {required_bytes} bytes are required for its pre-image: {error}",
        db_path.display(),
    )
}

async fn rebuild_v0_schema(
    pool: &SqlitePool,
    now_ms: i64,
    snapshot_json_keep_ms: i64,
    migration_audit: Option<&MigrationAudit>,
) -> Result<(), StoreError> {
    let cutoff_ms = now_ms.saturating_sub(snapshot_json_keep_ms);
    let mut transaction = pool.begin().await?;
    sqlx::query(CREATE_METRIC_SAMPLES_V1_TEMP_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(COPY_METRIC_SAMPLES_TO_V1_SQL)
        .bind(cutoff_ms)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DROP TABLE metric_samples")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("ALTER TABLE metric_samples_v1 RENAME TO metric_samples")
        .execute(&mut *transaction)
        .await?;
    sqlx::raw_sql(CREATE_SCHEMA_V1_SQL)
        .execute(&mut *transaction)
        .await?;
    add_rollup_1m_columns(&mut transaction).await?;
    if let Some(audit) = migration_audit {
        write_migration_state(&mut transaction, audit, now_ms).await?;
    }
    transaction.commit().await?;
    Ok(())
}

async fn add_rollup_1m_columns(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    for (column, alter_sql) in ROLLUP_1M_ADDITIVE_COLUMNS {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('metric_rollups_1m') WHERE name = ?",
        )
        .bind(column)
        .fetch_one(&mut **transaction)
        .await?;
        if exists == 0 {
            sqlx::query(alter_sql).execute(&mut **transaction).await?;
        }
    }
    Ok(())
}

async fn table_exists(pool: &SqlitePool, table: &str) -> Result<bool, StoreError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(table)
            .fetch_one(pool)
            .await?;
    Ok(count == 1)
}

pub fn canonical_database_path(db_path: &Path) -> Result<PathBuf, StoreError> {
    db_path
        .canonicalize()
        .map_err(|error| StoreError::Migration {
            reason: format!(
                "cannot resolve database path {} before migration: {error}",
                db_path.display()
            ),
            remedy: "make the database path accessible and retry; no migration was attempted"
                .to_string(),
        })
}

pub fn pre_image_path(db_path: &Path) -> PathBuf {
    let mut path = OsString::from(db_path.as_os_str());
    path.push(".pre-v0.sqlite");
    PathBuf::from(path)
}

fn refuse_existing_pre_image(pre_image_path: &Path) -> Result<(), StoreError> {
    match std::fs::symlink_metadata(pre_image_path) {
        Ok(_) => Err(StoreError::Migration {
            reason: format!(
                "pre-image path already exists: {}",
                pre_image_path.display()
            ),
            remedy: "move the existing pre-image to a safe location, then retry; it is never overwritten or deleted automatically"
                .to_string(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Migration {
            reason: format!(
                "cannot verify whether pre-image path {} already exists: {error}",
                pre_image_path.display()
            ),
            remedy: "make the pre-image directory readable and retry; no migration was attempted"
                .to_string(),
        }),
    }
}

async fn create_pre_image(pool: &SqlitePool, pre_image_path: &Path) -> Result<(), StoreError> {
    let Some(pre_image_sql_path) = pre_image_path.to_str() else {
        return Err(StoreError::Migration {
            reason: format!(
                "pre-image path is not valid UTF-8: {}",
                pre_image_path.display()
            ),
            remedy: "move the database to a UTF-8 path and retry; no migration was attempted"
                .to_string(),
        });
    };
    sqlx::query("VACUUM INTO ?")
        .bind(pre_image_sql_path)
        .execute(pool)
        .await
        .map_err(|error| StoreError::Migration {
            reason: format!(
                "failed to create complete pre-image {}: {error}",
                pre_image_path.display()
            ),
            remedy: "inspect and move any partial pre-image aside, make enough space available, and retry"
                .to_string(),
        })?;
    Ok(())
}

async fn complete_pending_migration(
    pool: &SqlitePool,
    db_path: &Path,
) -> Result<Option<MigrationReport>, StoreError> {
    let value_json: Option<String> = sqlx::query_scalar(
        "SELECT value_json FROM history_state WHERE state_key = 'schemaMigration'",
    )
    .fetch_optional(pool)
    .await?;
    let Some(value_json) = value_json else {
        return Ok(None);
    };
    let value: JsonValue = serde_json::from_str(&value_json)?;
    if !matches!(value.get("vacuumedAtMs"), Some(JsonValue::Null)) {
        return Ok(None);
    }

    let audit: MigrationAudit = serde_json::from_value(value)?;
    let canonical_db_path = db_path
        .canonicalize()
        .map_err(|error| StoreError::Migration {
            reason: format!(
                "schema reached v1 with an incomplete migration record, but database path {} cannot be resolved: {error}",
                db_path.display()
            ),
            remedy: "keep the pre-image, make the database path accessible, and retry startup"
                .to_string(),
        })?;
    let completed = vacuum_and_complete_migration(pool, &canonical_db_path, audit).await?;
    Ok(Some(completed.report))
}

async fn vacuum_and_complete_migration(
    pool: &SqlitePool,
    db_path: &Path,
    mut audit: MigrationAudit,
) -> Result<MigrationAudit, StoreError> {
    sqlx::query("VACUUM").execute(pool).await?;
    let vacuumed_at_ms = current_time_ms();
    let bytes_after = database_bytes_with_wal(db_path)?;
    audit.report.vacuumed_at_ms = Some(vacuumed_at_ms);
    audit.report.bytes_after = Some(bytes_to_i64(bytes_after, "post-migration database")?);
    audit.report.duration_ms = Some(vacuumed_at_ms.saturating_sub(audit.started_at_ms).max(0));
    finish_migration_audit(pool, &audit, vacuumed_at_ms).await?;
    Ok(audit)
}

async fn write_migration_state(
    transaction: &mut Transaction<'_, Sqlite>,
    audit: &MigrationAudit,
    updated_at_ms: i64,
) -> Result<(), StoreError> {
    let value_json = serde_json::to_string(&audit)?;
    sqlx::query(
        r#"
        INSERT INTO history_state (state_key, value_json, updated_at_ms)
        VALUES ('schemaMigration', ?, ?)
        ON CONFLICT(state_key) DO UPDATE SET
          value_json = excluded.value_json,
          updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(&value_json)
    .bind(updated_at_ms)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn finish_migration_audit(
    pool: &SqlitePool,
    audit: &MigrationAudit,
    vacuumed_at_ms: i64,
) -> Result<(), StoreError> {
    let value_json = serde_json::to_string(audit)?;
    let mut transaction = pool.begin().await?;
    write_migration_state(&mut transaction, audit, vacuumed_at_ms).await?;
    sqlx::query(
        r#"
        INSERT INTO app_events (occurred_at_ms, marker_type, label, details_json)
        SELECT ?, 'schemaMigrated', 'SQLite schema migrated from v0 to v1', ?
        WHERE NOT EXISTS (
          SELECT 1 FROM app_events WHERE marker_type = 'schemaMigrated'
        )
        "#,
    )
    .bind(vacuumed_at_ms)
    .bind(&value_json)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn bytes_to_i64(bytes: u64, description: &str) -> Result<i64, StoreError> {
    i64::try_from(bytes).map_err(|_| StoreError::Migration {
        reason: format!("{description} size {bytes} does not fit in SQLite INTEGER"),
        remedy: "keep the pre-image and inspect the database before retrying".to_string(),
    })
}

fn database_bytes_with_wal(canonical_db_path: &Path) -> Result<u64, StoreError> {
    let main_bytes = std::fs::symlink_metadata(canonical_db_path)
        .map_err(|error| StoreError::Migration {
            reason: format!(
                "database bytes check could not read the main file at {}: {error}",
                canonical_db_path.display()
            ),
            remedy: "make the database file readable and retry; if the schema phase already committed, keep the pre-image and retry startup"
                .to_string(),
        })?
        .len();
    let mut wal_path = OsString::from(canonical_db_path.as_os_str());
    wal_path.push("-wal");
    let wal_path = PathBuf::from(wal_path);
    let wal_bytes = match std::fs::symlink_metadata(&wal_path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => {
            return Err(StoreError::Migration {
                reason: format!(
                    "database WAL bytes check could not read {}: {error}",
                    wal_path.display()
                ),
                remedy: "make the database WAL path readable and retry; if the schema phase already committed, keep the pre-image and retry startup"
                    .to_string(),
            });
        }
    };
    main_bytes
        .checked_add(wal_bytes)
        .ok_or_else(|| StoreError::Migration {
            reason: format!(
                "database bytes check overflowed for observed main file {main_bytes} and WAL {wal_bytes} bytes"
            ),
            remedy: "keep the database and WAL unchanged and inspect their sizes before retrying"
                .to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn schema_group_failure_rolls_back_fresh_schema() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory fixture should connect");

        let error = apply_schema_groups(
            &pool,
            &[
                "CREATE TABLE partial_v2 (value INTEGER)",
                "this is not valid SQLite",
            ],
        )
        .await
        .expect_err("the injected schema failure should be returned");
        assert!(error.to_string().contains("syntax error"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'partial_v2'",
            )
            .fetch_one(&pool)
            .await
            .expect("schema should remain readable"),
            0
        );
    }

    #[test]
    fn database_bytes_with_wal_counts_the_main_file_and_optional_sidecar() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be after the epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-migration-wal-bytes-{}-{stamp}",
            std::process::id()
        ));
        assert!(directory.starts_with(std::env::temp_dir()));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let database_path = directory.join("history.sqlite");
        let wal_path = PathBuf::from(format!("{}-wal", database_path.display()));
        std::fs::write(&database_path, vec![0_u8; 4_096])
            .expect("fixture database should be written");

        assert_eq!(
            database_bytes_with_wal(&database_path).expect("main size should read"),
            4_096
        );

        std::fs::write(&wal_path, vec![0_u8; 8_192]).expect("fixture WAL should be written");
        assert_eq!(
            database_bytes_with_wal(&database_path).expect("main plus WAL size should read"),
            12_288
        );

        std::fs::remove_dir_all(&directory).expect("owned fixture directory should be removed");
    }

    #[test]
    fn undeterminable_free_space_reason_names_database_and_required_bytes() {
        let error = io::Error::new(io::ErrorKind::NotFound, "fixture mount lookup failed");

        let reason = undeterminable_free_space_reason(
            Path::new("/var/lib/tinytop/history.sqlite"),
            4_096,
            4_915,
            &error,
        );

        assert!(reason.contains("4096"), "missing database bytes: {reason}");
        assert!(reason.contains("4915"), "missing required bytes: {reason}");
    }
}
