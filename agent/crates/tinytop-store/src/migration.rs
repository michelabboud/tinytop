use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{Sqlite, SqlitePool, Transaction};
use tinytop_types::SystemSnapshot;

use crate::{StoreError, disk};

pub const SCHEMA_VERSION: i64 = 4;

/// Historical v0→v1 JSON retention window.
///
/// Schema v3 removes the configurable JSON tier, but v0 files must still pass
/// through the loss-bounded v1 shape before the atomic v2→v3 typed backfill.
const V0_JSON_KEEP_MS: i64 = 60 * 60 * 1_000;

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

#[doc(hidden)]
pub const CREATE_SCHEMA_V2_SQL: [&str; 5] = [
    CREATE_SCHEMA_V2_HEAD_SQL,
    CREATE_PROCESS_COMMANDS_V2_SQL,
    CREATE_PROCESS_SAMPLES_FAST_V2_SQL,
    CREATE_PROCESS_SAMPLES_V2_SQL,
    CREATE_SCHEMA_V2_TAIL_SQL,
];

const CREATE_SCHEMA_V3_HEAD_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS host_identity (
  identity_id INTEGER PRIMARY KEY,
  first_seen_ms INTEGER NOT NULL,
  hostname TEXT NOT NULL,
  platform TEXT NOT NULL,
  arch TEXT NOT NULL,
  distro TEXT NOT NULL,
  kernel TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  runtime_confidence TEXT NOT NULL,
  runtime_reason TEXT NOT NULL,
  UNIQUE (
    hostname, platform, arch, distro, kernel,
    runtime_kind, runtime_confidence, runtime_reason
  )
);

CREATE TABLE IF NOT EXISTS fs_mount_events (
  captured_at_ms INTEGER NOT NULL,
  mount TEXT NOT NULL,
  present INTEGER NOT NULL CHECK (present IN (0, 1)),
  PRIMARY KEY (mount, captured_at_ms)
) WITHOUT ROWID;

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
  runnable_threads INTEGER,
  total_threads INTEGER,
  root_used_percent REAL,
  identity_id INTEGER REFERENCES host_identity(identity_id),
  uptime_seconds INTEGER,
  memory_available_bytes INTEGER,
  swap_free_bytes INTEGER,
  last_pid INTEGER,
  filesystems_captured_at_ms INTEGER,
  CHECK (
    identity_id IS NULL OR (
      uptime_seconds IS NOT NULL
      AND memory_available_bytes IS NOT NULL
      AND swap_free_bytes IS NOT NULL
    )
  )
);
"#;

const CREATE_SCHEMA_V3_TAIL_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);

PRAGMA user_version = 3;
"#;

#[doc(hidden)]
pub const CREATE_SCHEMA_V3_SQL: [&str; 6] = [
    CREATE_SCHEMA_V3_HEAD_SQL,
    CREATE_SCHEMA_V2_HEAD_SQL,
    CREATE_PROCESS_COMMANDS_V2_SQL,
    CREATE_PROCESS_SAMPLES_FAST_V2_SQL,
    CREATE_PROCESS_SAMPLES_V2_SQL,
    CREATE_SCHEMA_V3_TAIL_SQL,
];

const CREATE_PROCESS_SAMPLES_FAST_V4_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS process_samples_fast (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command_id INTEGER NOT NULL REFERENCES process_commands(command_id),
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at_ms INTEGER,
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_process_samples_fast_command
  ON process_samples_fast (command_id);
"#;

const CREATE_PROCESS_SAMPLES_V4_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS process_samples (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at_ms INTEGER,
  command_id INTEGER REFERENCES process_commands(command_id),
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
);

CREATE INDEX IF NOT EXISTS idx_process_samples_time
  ON process_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_process_samples_command
  ON process_samples (command_id);
"#;

const CREATE_GPU_TABLES_V4_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS gpu_adapters (
  adapter_id INTEGER PRIMARY KEY,
  stable_id TEXT NOT NULL UNIQUE,
  vendor TEXT NOT NULL,
  name TEXT NOT NULL,
  driver TEXT NOT NULL,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS gpu_samples (
  captured_at_ms INTEGER NOT NULL,
  adapter_id INTEGER NOT NULL REFERENCES gpu_adapters(adapter_id),
  busy_percent REAL,
  memory_used_bytes INTEGER,
  memory_total_bytes INTEGER,
  temperature_c REAL,
  PRIMARY KEY (captured_at_ms, adapter_id)
) WITHOUT ROWID;
"#;

const CREATE_SCHEMA_V4_TAIL_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);

PRAGMA user_version = 4;
"#;

#[doc(hidden)]
pub const CREATE_SCHEMA_V4_SQL: [&str; 7] = [
    CREATE_SCHEMA_V3_HEAD_SQL,
    CREATE_SCHEMA_V2_HEAD_SQL,
    CREATE_PROCESS_COMMANDS_V2_SQL,
    CREATE_PROCESS_SAMPLES_FAST_V4_SQL,
    CREATE_PROCESS_SAMPLES_V4_SQL,
    CREATE_GPU_TABLES_V4_SQL,
    CREATE_SCHEMA_V4_TAIL_SQL,
];

const CREATE_PROCESS_SAMPLES_FAST_V4_TEMP_SQL: &str = r#"
CREATE TABLE process_samples_fast_v4 (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command_id INTEGER NOT NULL REFERENCES process_commands(command_id),
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at_ms INTEGER,
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
) WITHOUT ROWID
"#;

const CREATE_PROCESS_SAMPLES_V4_TEMP_SQL: &str = r#"
CREATE TABLE process_samples_v4 (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at_ms INTEGER,
  command_id INTEGER REFERENCES process_commands(command_id),
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
)
"#;

const CREATE_HOST_IDENTITY_V3_SQL: &str = r#"
CREATE TABLE host_identity (
  identity_id INTEGER PRIMARY KEY,
  first_seen_ms INTEGER NOT NULL,
  hostname TEXT NOT NULL,
  platform TEXT NOT NULL,
  arch TEXT NOT NULL,
  distro TEXT NOT NULL,
  kernel TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  runtime_confidence TEXT NOT NULL,
  runtime_reason TEXT NOT NULL,
  UNIQUE (
    hostname, platform, arch, distro, kernel,
    runtime_kind, runtime_confidence, runtime_reason
  )
)
"#;

const CREATE_FS_MOUNT_EVENTS_V3_SQL: &str = r#"
CREATE TABLE fs_mount_events (
  captured_at_ms INTEGER NOT NULL,
  mount TEXT NOT NULL,
  present INTEGER NOT NULL CHECK (present IN (0, 1)),
  PRIMARY KEY (mount, captured_at_ms)
) WITHOUT ROWID
"#;

const CREATE_METRIC_SAMPLES_V3_TEMP_SQL: &str = r#"
CREATE TABLE metric_samples_v3 (
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
  runnable_threads INTEGER,
  total_threads INTEGER,
  root_used_percent REAL,
  identity_id INTEGER REFERENCES host_identity(identity_id),
  uptime_seconds INTEGER,
  memory_available_bytes INTEGER,
  swap_free_bytes INTEGER,
  last_pid INTEGER,
  filesystems_captured_at_ms INTEGER,
  CHECK (
    identity_id IS NULL OR (
      uptime_seconds IS NOT NULL
      AND memory_available_bytes IS NOT NULL
      AND swap_free_bytes IS NOT NULL
    )
  )
)
"#;

const COPY_METRIC_SAMPLES_TO_V3_SQL: &str = r#"
INSERT INTO metric_samples_v3 (
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
  root_used_percent
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
  root_used_percent
FROM metric_samples
"#;

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
) -> Result<Option<MigrationReport>, StoreError> {
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;

    match user_version {
        SCHEMA_VERSION => {
            apply_schema_v4(pool).await?;
            complete_pending_migration(pool, db_path).await
        }
        3 => {
            migrate_v3_to_v4(pool, now_ms).await?;
            complete_pending_migration(pool, db_path).await
        }
        2 => {
            migrate_v2_to_v3(pool, now_ms).await?;
            migrate_v3_to_v4(pool, now_ms).await?;
            complete_pending_migration(pool, db_path).await
        }
        1 => {
            let report = complete_pending_migration(pool, db_path).await?;
            migrate_v1_to_v2(pool, db_path, now_ms).await?;
            migrate_v2_to_v3(pool, now_ms).await?;
            migrate_v3_to_v4(pool, now_ms).await?;
            Ok(report)
        }
        0 => {
            let metric_samples_exists = table_exists(pool, "metric_samples").await?;
            if !metric_samples_exists {
                apply_schema_v4(pool).await?;
                return Ok(None);
            }

            let sample_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples")
                .fetch_one(pool)
                .await?;
            if sample_count == 0 {
                rebuild_v0_schema(pool, now_ms, V0_JSON_KEEP_MS, None).await?;
                migrate_v1_to_v2(pool, db_path, now_ms).await?;
                migrate_v2_to_v3(pool, now_ms).await?;
                migrate_v3_to_v4(pool, now_ms).await?;
                return Ok(None);
            }

            let report =
                migrate_populated_v0(pool, db_path, now_ms, V0_JSON_KEEP_MS, sample_count).await?;
            migrate_v1_to_v2(pool, db_path, now_ms).await?;
            migrate_v2_to_v3(pool, now_ms).await?;
            migrate_v3_to_v4(pool, now_ms).await?;
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

async fn apply_schema_v4(pool: &SqlitePool) -> Result<(), StoreError> {
    apply_schema_groups(pool, &CREATE_SCHEMA_V4_SQL).await
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

async fn migrate_v2_to_v3(pool: &SqlitePool, now_ms: i64) -> Result<(), StoreError> {
    let started = Instant::now();
    let mut transaction = pool.begin().await?;

    sqlx::query(CREATE_HOST_IDENTITY_V3_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(CREATE_FS_MOUNT_EVENTS_V3_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(CREATE_METRIC_SAMPLES_V3_TEMP_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(COPY_METRIC_SAMPLES_TO_V3_SQL)
        .execute(&mut *transaction)
        .await?;

    let json_rows = sqlx::query_as::<_, (i64, i64, String)>(
        r#"
        SELECT sample_id, captured_at_ms, snapshot_json
        FROM metric_samples
        WHERE snapshot_json IS NOT NULL
        ORDER BY sample_id
        "#,
    )
    .fetch_all(&mut *transaction)
    .await?;
    let json_rows_decoded = i64::try_from(json_rows.len()).map_err(|_| StoreError::Migration {
        reason: "schema v3 JSON row count exceeds SQLite INTEGER capacity".to_string(),
        remedy: "inspect the database with `db check`; the database was not modified".to_string(),
    })?;
    let mut legacy_inode_rows_normalised = 0_i64;

    for (sample_id, captured_at_ms, snapshot_json) in json_rows {
        let mut value = serde_json::from_str::<serde_json::Value>(&snapshot_json).map_err(
            |error| StoreError::Migration {
                reason: format!(
                    "metric_samples row {sample_id} holds snapshot JSON that does not decode: {error}"
                ),
                remedy: "a row this version cannot decode — back up the database, then clear that row's payload (UPDATE metric_samples SET snapshot_json = NULL WHERE sample_id = <n>; see INSTALL.md, Upgrade) and start again; the database was not modified".to_string(),
            },
        )?;
        if normalise_legacy_snapshot(&mut value) > 0 {
            legacy_inode_rows_normalised += 1;
        }
        let snapshot = serde_json::from_value::<SystemSnapshot>(value).map_err(|error| {
            StoreError::Migration {
                reason: format!(
                    "metric_samples row {sample_id} holds snapshot JSON that does not decode: {error}"
                ),
                remedy: "a row this version cannot decode — back up the database, then clear that row's payload (UPDATE metric_samples SET snapshot_json = NULL WHERE sample_id = <n>; see INSTALL.md, Upgrade) and start again; the database was not modified".to_string(),
            }
        })?;
        let identity = &snapshot.identity;
        let runtime = &identity.runtime;
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO host_identity (
              first_seen_ms, hostname, platform, arch, distro, kernel,
              runtime_kind, runtime_confidence, runtime_reason
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(captured_at_ms)
        .bind(&identity.hostname)
        .bind(&identity.platform)
        .bind(&identity.arch)
        .bind(&identity.distro)
        .bind(&identity.kernel)
        .bind(runtime.kind.as_str())
        .bind(runtime.confidence.as_str())
        .bind(&runtime.reason)
        .execute(&mut *transaction)
        .await?;

        let identity_id: i64 = sqlx::query_scalar(
            r#"
            SELECT identity_id
            FROM host_identity
            WHERE hostname = ?
              AND platform = ?
              AND arch = ?
              AND distro = ?
              AND kernel = ?
              AND runtime_kind = ?
              AND runtime_confidence = ?
              AND runtime_reason = ?
            "#,
        )
        .bind(&identity.hostname)
        .bind(&identity.platform)
        .bind(&identity.arch)
        .bind(&identity.distro)
        .bind(&identity.kernel)
        .bind(runtime.kind.as_str())
        .bind(runtime.confidence.as_str())
        .bind(&runtime.reason)
        .fetch_one(&mut *transaction)
        .await?;

        let uptime_seconds =
            migration_u64_to_i64(identity.uptime_seconds, sample_id, "identity.uptimeSeconds")?;
        let memory_available_bytes = migration_u64_to_i64(
            snapshot.memory.available_bytes,
            sample_id,
            "memory.availableBytes",
        )?;
        let swap_free_bytes =
            migration_u64_to_i64(snapshot.swap.free_bytes, sample_id, "swap.freeBytes")?;
        let last_pid = snapshot
            .load
            .last_pid
            .map(|value| migration_u64_to_i64(value, sample_id, "load.lastPid"))
            .transpose()?;

        sqlx::query(
            r#"
            UPDATE metric_samples_v3
            SET identity_id = ?,
                uptime_seconds = ?,
                memory_available_bytes = ?,
                swap_free_bytes = ?,
                last_pid = ?,
                filesystems_captured_at_ms = ?
            WHERE sample_id = ?
            "#,
        )
        .bind(identity_id)
        .bind(uptime_seconds)
        .bind(memory_available_bytes)
        .bind(swap_free_bytes)
        .bind(last_pid)
        .bind(snapshot.filesystems_captured_at_ms)
        .bind(sample_id)
        .execute(&mut *transaction)
        .await?;
    }

    let assembleable_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples_v3 WHERE identity_id IS NOT NULL")
            .fetch_one(&mut *transaction)
            .await?;
    if assembleable_rows != json_rows_decoded {
        return Err(StoreError::Migration {
            reason: format!(
                "schema v3 backfill decoded {json_rows_decoded} JSON rows but made {assembleable_rows} metric_samples rows assembleable"
            ),
            remedy: "inspect the database with `db check`; the database was not modified"
                .to_string(),
        });
    }

    let sample_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples_v3")
        .fetch_one(&mut *transaction)
        .await?;
    let identities_interned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM host_identity")
        .fetch_one(&mut *transaction)
        .await?;

    sqlx::query("DROP TABLE metric_samples")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("ALTER TABLE metric_samples_v3 RENAME TO metric_samples")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "CREATE INDEX idx_metric_samples_captured_at ON metric_samples (captured_at_ms DESC)",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "CREATE INDEX idx_metric_samples_runtime_captured_at ON metric_samples (runtime_kind, captured_at_ms DESC)",
    )
    .execute(&mut *transaction)
    .await?;

    let appear_events = sqlx::query(
        r#"
        INSERT INTO fs_mount_events (captured_at_ms, mount, present)
        SELECT MIN(captured_at_ms), mount, 1
        FROM fs_samples
        GROUP BY mount
        "#,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    let disappear_events = sqlx::query(
        r#"
        INSERT INTO fs_mount_events (captured_at_ms, mount, present)
        SELECT MAX(captured_at_ms) + 1, mount, 0
        FROM fs_samples
        GROUP BY mount
        HAVING MAX(captured_at_ms) < (SELECT MAX(captured_at_ms) FROM fs_samples)
        "#,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    let events_written =
        i64::try_from(appear_events.saturating_add(disappear_events)).map_err(|_| {
            StoreError::Migration {
                reason: "schema v3 filesystem event count exceeds SQLite INTEGER capacity"
                    .to_string(),
                remedy: "inspect the database with `db check`; the database was not modified"
                    .to_string(),
            }
        })?;

    let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let details_json = serde_json::to_string(&serde_json::json!({
        "fromVersion": 2,
        "toVersion": 3,
        "sampleRows": sample_rows,
        "jsonRowsDecoded": json_rows_decoded,
        "legacyInodeRowsNormalised": legacy_inode_rows_normalised,
        "identitiesInterned": identities_interned,
        "eventsWritten": events_written,
        "durationMs": duration_ms,
    }))?;
    sqlx::query(
        r#"
        INSERT INTO app_events (occurred_at_ms, marker_type, label, details_json)
        VALUES (?, 'schemaMigrated', 'SQLite schema migrated from v2 to v3', ?)
        "#,
    )
    .bind(now_ms)
    .bind(details_json)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("PRAGMA user_version = 3")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    eprintln!(
        "history migration info: schema v2 → v3 in {duration_ms} ms ({json_rows_decoded} JSON rows decoded, {identities_interned} identities interned, {events_written} filesystem events written over {sample_rows} metric rows, {legacy_inode_rows_normalised} rows with legacy negative inode counts normalised)"
    );
    Ok(())
}

async fn migrate_v3_to_v4(pool: &SqlitePool, now_ms: i64) -> Result<(), StoreError> {
    let started = Instant::now();
    let mut transaction = pool.begin().await?;

    sqlx::query(CREATE_PROCESS_SAMPLES_FAST_V4_TEMP_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(CREATE_PROCESS_SAMPLES_V4_TEMP_SQL)
        .execute(&mut *transaction)
        .await?;
    sqlx::raw_sql(CREATE_GPU_TABLES_V4_SQL)
        .execute(&mut *transaction)
        .await?;

    let fast_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM process_samples_fast")
        .fetch_one(&mut *transaction)
        .await?;
    let minute_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM process_samples")
        .fetch_one(&mut *transaction)
        .await?;
    let fast_unparsed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_samples_fast WHERE started_at IS NOT NULL AND strftime('%s', started_at) IS NULL",
    )
    .fetch_one(&mut *transaction)
    .await?;
    let minute_unparsed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_samples WHERE started_at IS NOT NULL AND strftime('%s', started_at) IS NULL",
    )
    .fetch_one(&mut *transaction)
    .await?;
    let started_at_unparsed = fast_unparsed.checked_add(minute_unparsed).ok_or_else(|| {
        StoreError::Migration {
            reason: "schema v4 unparsable start-time count exceeds SQLite INTEGER capacity"
                .to_string(),
            remedy: "inspect the database with `db check`; the database was not modified"
                .to_string(),
        }
    })?;

    sqlx::query(
        r#"
        INSERT INTO process_samples_fast_v4 (
          captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
          rss_bytes, parent_pid, started_at_ms, gpu_percent
        )
        SELECT captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent,
               rss_bytes, parent_pid,
               CAST(strftime('%s', started_at) AS INTEGER) * 1000,
               gpu_percent
        FROM process_samples_fast
        "#,
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO process_samples_v4 (
          captured_at_ms, rank, pid, cpu_percent, memory_percent, rss_bytes,
          parent_pid, started_at_ms, command_id, gpu_percent
        )
        SELECT captured_at_ms, rank, pid, cpu_percent, memory_percent, rss_bytes,
               parent_pid, CAST(strftime('%s', started_at) AS INTEGER) * 1000,
               command_id, NULL
        FROM process_samples
        "#,
    )
    .execute(&mut *transaction)
    .await?;

    let copied_fast_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM process_samples_fast_v4")
            .fetch_one(&mut *transaction)
            .await?;
    if copied_fast_rows != fast_rows {
        return Err(StoreError::Migration {
            reason: format!(
                "schema v4 rebuild copied {copied_fast_rows} of {fast_rows} process_samples_fast rows"
            ),
            remedy: "inspect the database with `db check`; the database was not modified"
                .to_string(),
        });
    }
    let copied_minute_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM process_samples_v4")
        .fetch_one(&mut *transaction)
        .await?;
    if copied_minute_rows != minute_rows {
        return Err(StoreError::Migration {
            reason: format!(
                "schema v4 rebuild copied {copied_minute_rows} of {minute_rows} process_samples rows"
            ),
            remedy: "inspect the database with `db check`; the database was not modified"
                .to_string(),
        });
    }

    sqlx::query("DROP TABLE process_samples_fast")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("ALTER TABLE process_samples_fast_v4 RENAME TO process_samples_fast")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "CREATE INDEX idx_process_samples_fast_command ON process_samples_fast (command_id)",
    )
    .execute(&mut *transaction)
    .await?;

    sqlx::query("DROP TABLE process_samples")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("ALTER TABLE process_samples_v4 RENAME TO process_samples")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "CREATE INDEX idx_process_samples_time ON process_samples (captured_at_ms DESC)",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query("CREATE INDEX idx_process_samples_command ON process_samples (command_id)")
        .execute(&mut *transaction)
        .await?;

    let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let details_json = serde_json::to_string(&serde_json::json!({
        "fromVersion": 3,
        "toVersion": 4,
        "fastRows": fast_rows,
        "minuteRows": minute_rows,
        "startedAtUnparsed": started_at_unparsed,
        "durationMs": duration_ms,
    }))?;
    sqlx::query(
        r#"
        INSERT INTO app_events (occurred_at_ms, marker_type, label, details_json)
        VALUES (?, 'schemaMigrated', 'SQLite schema migrated from v3 to v4', ?)
        "#,
    )
    .bind(now_ms)
    .bind(details_json)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("PRAGMA user_version = 4")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    eprintln!(
        "history migration info: schema v3 → v4 in {duration_ms} ms ({fast_rows} fast process rows and {minute_rows} minute process rows rebuilt with started_at_ms, {started_at_unparsed} unparsable start times stored as NULL; gpu_adapters and gpu_samples created)"
    );
    Ok(())
}

/// Normalises negative inode counts emitted by the legacy Bun collector when
/// its unclamped `inodeTotal - inodeFree` subtraction observed more free inodes
/// than total inodes. The return value counts fields changed, not rows.
fn normalise_legacy_snapshot(value: &mut serde_json::Value) -> u32 {
    let Some(filesystems) = value
        .get_mut("filesystems")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let mut fields_changed = 0_u32;
    for filesystem in filesystems {
        let Some(filesystem) = filesystem.as_object_mut() else {
            continue;
        };
        for field in ["inodeUsed", "inodeTotal"] {
            let Some(field_value) = filesystem.get_mut(field) else {
                continue;
            };
            if field_value.as_i64().is_some_and(|value| value < 0) {
                *field_value = serde_json::Value::Null;
                fields_changed += 1;
            }
        }
    }
    fields_changed
}

fn migration_u64_to_i64(
    value: u64,
    sample_id: i64,
    field: &'static str,
) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Migration {
        reason: format!(
            "metric_samples row {sample_id} holds {field} value {value} that does not fit SQLite INTEGER"
        ),
        remedy: "a row this version cannot decode — back up the database, then clear that row's payload (UPDATE metric_samples SET snapshot_json = NULL WHERE sample_id = <n>; see INSTALL.md, Upgrade) and start again; the database was not modified".to_string(),
    })
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
    v0_json_keep_ms: i64,
    sample_count: i64,
) -> Result<MigrationReport, StoreError> {
    let (canonical_db_path, audit) = migrate_populated_v0_schema_phase_inner(
        pool,
        db_path,
        now_ms,
        v0_json_keep_ms,
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
    v0_json_keep_ms: i64,
) -> Result<MigrationReport, StoreError> {
    let sample_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metric_samples")
        .fetch_one(pool)
        .await?;
    let (_, audit) = migrate_populated_v0_schema_phase_inner(
        pool,
        db_path,
        now_ms,
        v0_json_keep_ms,
        sample_count,
    )
    .await?;
    Ok(audit.report.clone())
}

async fn migrate_populated_v0_schema_phase_inner(
    pool: &SqlitePool,
    db_path: &Path,
    now_ms: i64,
    v0_json_keep_ms: i64,
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

    let cutoff_ms = now_ms.saturating_sub(v0_json_keep_ms);
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
    rebuild_v0_schema(pool, now_ms, v0_json_keep_ms, Some(&audit)).await?;
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
    v0_json_keep_ms: i64,
    migration_audit: Option<&MigrationAudit>,
) -> Result<(), StoreError> {
    let cutoff_ms = now_ms.saturating_sub(v0_json_keep_ms);
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

    #[test]
    fn normalise_legacy_snapshot_nulls_negative_inode_fields_and_counts_fields() {
        let mut only_inode_used = serde_json::json!({
            "filesystems": [{ "inodeUsed": -999001, "inodeTotal": 999 }],
        });
        assert_eq!(normalise_legacy_snapshot(&mut only_inode_used), 1);
        assert_eq!(
            only_inode_used["filesystems"][0]["inodeUsed"],
            JsonValue::Null
        );
        assert_eq!(only_inode_used["filesystems"][0]["inodeTotal"], 999);

        let mut both = serde_json::json!({
            "filesystems": [{ "inodeUsed": -1, "inodeTotal": -2 }],
        });
        assert_eq!(normalise_legacy_snapshot(&mut both), 2);
        assert_eq!(both["filesystems"][0]["inodeUsed"], JsonValue::Null);
        assert_eq!(both["filesystems"][0]["inodeTotal"], JsonValue::Null);
    }

    #[test]
    fn normalise_legacy_snapshot_leaves_other_inode_values_byte_identical() {
        let mut value = serde_json::json!({
            "filesystems": [
                { "inodeUsed": 0, "inodeTotal": 1 },
                { "inodeUsed": null, "inodeTotal": null },
                { "inodeUsed": -1.5, "inodeTotal": -2.5 },
                { "inodeUsed": "x", "inodeTotal": "x" },
                { "inodeUsed": true, "inodeTotal": true },
                { "inodeTotal": 999 },
                { "inodeUsed": 999 }
            ],
        });
        let before = serde_json::to_vec(&value).expect("fixture JSON should serialize");

        assert_eq!(normalise_legacy_snapshot(&mut value), 0);
        assert_eq!(
            serde_json::to_vec(&value).expect("normalised JSON should serialize"),
            before
        );
    }

    #[test]
    fn normalise_legacy_snapshot_leaves_other_filesystem_shapes_byte_identical() {
        for mut value in [
            serde_json::json!({ "identity": {} }),
            serde_json::json!({ "filesystems": null }),
            serde_json::json!({ "filesystems": { "inodeUsed": -1 } }),
            serde_json::json!({ "filesystems": [null, 1, "x", true, []] }),
        ] {
            let before = serde_json::to_vec(&value).expect("fixture JSON should serialize");
            assert_eq!(normalise_legacy_snapshot(&mut value), 0);
            assert_eq!(
                serde_json::to_vec(&value).expect("normalised JSON should serialize"),
                before
            );
        }
    }

    #[test]
    fn normalise_legacy_snapshot_counts_fields_across_filesystems() {
        let mut value = serde_json::json!({
            "filesystems": [
                { "mount": "/a", "inodeUsed": -1, "inodeTotal": 10 },
                { "mount": "/b", "inodeUsed": 10, "inodeTotal": -1 }
            ],
        });

        assert_eq!(normalise_legacy_snapshot(&mut value), 2);
        assert_eq!(value["filesystems"][0]["inodeUsed"], JsonValue::Null);
        assert_eq!(value["filesystems"][1]["inodeTotal"], JsonValue::Null);
    }

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
