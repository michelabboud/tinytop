use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tinytop_types::SystemSnapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySample {
    pub captured_at_ms: i64,
    pub snapshot: SystemSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStats {
    pub sample_count: i64,
    pub oldest_captured_at_ms: Option<i64>,
    pub newest_captured_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSettings {
    pub default_theme: String,
    pub default_graph_mode: String,
    pub poll_interval_ms: i64,
    pub default_history_window: String,
    pub retention_hours: i64,
    pub rollup_retention_days: i64,
    pub top_process_count: i64,
    pub redaction_default: bool,
    pub thresholds: DashboardThresholds,
    pub enabled_sections: DashboardSections,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardThresholds {
    pub cpu_warn: i64,
    #[serde(default = "default_cpu_critical")]
    pub cpu_critical: i64,
    pub memory_warn: i64,
    #[serde(default = "default_memory_critical")]
    pub memory_critical: i64,
    pub disk_warn: i64,
    #[serde(default = "default_disk_critical")]
    pub disk_critical: i64,
    #[serde(default = "default_load_warn")]
    pub load_warn: i64,
    #[serde(default = "default_load_critical")]
    pub load_critical: i64,
    #[serde(default = "default_pressure_warn")]
    pub pressure_warn: i64,
    #[serde(default = "default_pressure_critical")]
    pub pressure_critical: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSections {
    pub overview: bool,
    pub history: bool,
    pub filesystem: bool,
    pub pressure: bool,
    pub processes: bool,
}

impl Default for DashboardSettings {
    fn default() -> Self {
        Self {
            default_theme: "midnight".to_string(),
            default_graph_mode: "line".to_string(),
            poll_interval_ms: 1_500,
            default_history_window: "live".to_string(),
            retention_hours: 72,
            rollup_retention_days: 30,
            top_process_count: 8,
            redaction_default: false,
            thresholds: DashboardThresholds::default(),
            enabled_sections: DashboardSections::default(),
        }
    }
}

impl Default for DashboardThresholds {
    fn default() -> Self {
        Self {
            cpu_warn: 80,
            cpu_critical: default_cpu_critical(),
            memory_warn: 85,
            memory_critical: default_memory_critical(),
            disk_warn: 85,
            disk_critical: default_disk_critical(),
            load_warn: default_load_warn(),
            load_critical: default_load_critical(),
            pressure_warn: default_pressure_warn(),
            pressure_critical: default_pressure_critical(),
        }
    }
}

impl Default for DashboardSections {
    fn default() -> Self {
        Self {
            overview: true,
            history: true,
            filesystem: true,
            pressure: true,
            processes: true,
        }
    }
}

impl DashboardSettings {
    pub fn validate(&self) -> Result<(), StoreError> {
        validate_one_of(
            "defaultTheme",
            &self.default_theme,
            &["midnight", "matrix", "aurora", "solar", "ember"],
        )?;
        validate_one_of(
            "defaultGraphMode",
            &self.default_graph_mode,
            &["line", "area", "bar", "heatmap", "treemap"],
        )?;
        validate_one_of(
            "defaultHistoryWindow",
            &self.default_history_window,
            &["live", "15m", "1h", "6h", "24h"],
        )?;
        validate_range("pollIntervalMs", self.poll_interval_ms, 250, 60_000)?;
        validate_range("retentionHours", self.retention_hours, 1, 8_760)?;
        validate_range("rollupRetentionDays", self.rollup_retention_days, 1, 366)?;
        validate_range("topProcessCount", self.top_process_count, 1, 50)?;
        validate_range("thresholds.cpuWarn", self.thresholds.cpu_warn, 0, 100)?;
        validate_range(
            "thresholds.cpuCritical",
            self.thresholds.cpu_critical,
            0,
            100,
        )?;
        validate_range("thresholds.memoryWarn", self.thresholds.memory_warn, 0, 100)?;
        validate_range(
            "thresholds.memoryCritical",
            self.thresholds.memory_critical,
            0,
            100,
        )?;
        validate_range("thresholds.diskWarn", self.thresholds.disk_warn, 0, 100)?;
        validate_range(
            "thresholds.diskCritical",
            self.thresholds.disk_critical,
            0,
            100,
        )?;
        validate_range("thresholds.loadWarn", self.thresholds.load_warn, 0, 100)?;
        validate_range(
            "thresholds.loadCritical",
            self.thresholds.load_critical,
            0,
            100,
        )?;
        validate_range(
            "thresholds.pressureWarn",
            self.thresholds.pressure_warn,
            0,
            100,
        )?;
        validate_range(
            "thresholds.pressureCritical",
            self.thresholds.pressure_critical,
            0,
            100,
        )?;
        validate_threshold_pair(
            "thresholds.cpu",
            self.thresholds.cpu_warn,
            self.thresholds.cpu_critical,
        )?;
        validate_threshold_pair(
            "thresholds.memory",
            self.thresholds.memory_warn,
            self.thresholds.memory_critical,
        )?;
        validate_threshold_pair(
            "thresholds.disk",
            self.thresholds.disk_warn,
            self.thresholds.disk_critical,
        )?;
        validate_threshold_pair(
            "thresholds.load",
            self.thresholds.load_warn,
            self.thresholds.load_critical,
        )?;
        validate_threshold_pair(
            "thresholds.pressure",
            self.thresholds.pressure_warn,
            self.thresholds.pressure_critical,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCoverage {
    pub sample_count: i64,
    pub oldest_captured_at_ms: Option<i64>,
    pub newest_captured_at_ms: Option<i64>,
    pub retention_hours: i64,
    pub rollup_retention_days: i64,
    pub rollup_bucket_count: i64,
    pub database_bytes: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryQuery {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SqliteHistoryStore {
    pool: SqlitePool,
}

impl SqliteHistoryStore {
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let store = Self { pool };
        store.apply_schema().await?;
        Ok(store)
    }

    pub async fn get_settings(&self) -> Result<DashboardSettings, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT value_json
            FROM app_settings
            WHERE setting_key = 'dashboard'
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(DashboardSettings::default());
        };

        let value_json = row.try_get::<String, _>("value_json")?;
        let settings: DashboardSettings = serde_json::from_str(&value_json)?;
        settings.validate()?;
        Ok(settings)
    }

    pub async fn put_settings(
        &self,
        settings: &DashboardSettings,
    ) -> Result<DashboardSettings, StoreError> {
        settings.validate()?;
        let value_json = serde_json::to_string(settings)?;
        sqlx::query(
            r#"
            INSERT INTO app_settings (setting_key, value_json, updated_at_ms)
            VALUES ('dashboard', ?, ?)
            ON CONFLICT(setting_key) DO UPDATE SET
              value_json = excluded.value_json,
              updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(value_json)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;

        Ok(settings.clone())
    }

    pub async fn insert_snapshot(
        &self,
        captured_at_ms: i64,
        snapshot: &SystemSnapshot,
    ) -> Result<HistorySample, StoreError> {
        let snapshot_json = serde_json::to_string(snapshot)?;
        let root_used_percent = snapshot
            .filesystems
            .iter()
            .find(|filesystem| filesystem.mount == "/")
            .map(|filesystem| filesystem.used_percent);

        sqlx::query(
            r#"
            INSERT INTO metric_samples (
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
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(captured_at_ms) DO UPDATE SET
              snapshot_timestamp = excluded.snapshot_timestamp,
              hostname = excluded.hostname,
              runtime_kind = excluded.runtime_kind,
              cpu_usage_percent = excluded.cpu_usage_percent,
              cpu_cores = excluded.cpu_cores,
              memory_used_percent = excluded.memory_used_percent,
              memory_used_bytes = excluded.memory_used_bytes,
              memory_total_bytes = excluded.memory_total_bytes,
              swap_used_percent = excluded.swap_used_percent,
              swap_used_bytes = excluded.swap_used_bytes,
              swap_total_bytes = excluded.swap_total_bytes,
              load_one = excluded.load_one,
              load_five = excluded.load_five,
              load_fifteen = excluded.load_fifteen,
              load_percent = excluded.load_percent,
              runnable_threads = excluded.runnable_threads,
              total_threads = excluded.total_threads,
              root_used_percent = excluded.root_used_percent,
              snapshot_json = excluded.snapshot_json
            "#,
        )
        .bind(captured_at_ms)
        .bind(&snapshot.timestamp)
        .bind(&snapshot.identity.hostname)
        .bind(format!("{:?}", snapshot.identity.runtime.kind))
        .bind(snapshot.cpu.usage_percent)
        .bind(to_i64(snapshot.cpu.cores, "cpu cores")?)
        .bind(snapshot.memory.used_percent)
        .bind(to_i64(snapshot.memory.used_bytes, "memory used bytes")?)
        .bind(to_i64(snapshot.memory.total_bytes, "memory total bytes")?)
        .bind(snapshot.swap.used_percent)
        .bind(to_i64(snapshot.swap.used_bytes, "swap used bytes")?)
        .bind(to_i64(snapshot.swap.total_bytes, "swap total bytes")?)
        .bind(snapshot.load.one)
        .bind(snapshot.load.five)
        .bind(snapshot.load.fifteen)
        .bind(load_percent(snapshot))
        .bind(to_i64(snapshot.load.runnable, "runnable threads")?)
        .bind(to_i64(snapshot.load.total_threads, "total threads")?)
        .bind(root_used_percent)
        .bind(&snapshot_json)
        .execute(&self.pool)
        .await?;

        self.rebuild_rollup_bucket(bucket_start_ms(captured_at_ms))
            .await?;

        Ok(HistorySample {
            captured_at_ms,
            snapshot: snapshot.clone(),
        })
    }

    pub async fn latest_snapshot(&self) -> Result<Option<HistorySample>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT captured_at_ms, snapshot_json
            FROM metric_samples
            ORDER BY captured_at_ms DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_sample).transpose()
    }

    pub async fn read_history(
        &self,
        query: HistoryQuery,
    ) -> Result<Vec<HistorySample>, StoreError> {
        let limit = query.limit.unwrap_or(120).clamp(1, 10_000);
        let rows = sqlx::query(
            r#"
            SELECT captured_at_ms, snapshot_json
            FROM metric_samples
            WHERE (?1 IS NULL OR captured_at_ms >= ?1)
              AND (?2 IS NULL OR captured_at_ms <= ?2)
            ORDER BY captured_at_ms DESC
            LIMIT ?3
            "#,
        )
        .bind(query.since_ms)
        .bind(query.until_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut samples = rows
            .into_iter()
            .map(row_to_sample)
            .collect::<Result<Vec<_>, _>>()?;
        samples.reverse();
        Ok(samples)
    }

    pub async fn stats(&self) -> Result<StoreStats, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
              COUNT(*) AS sample_count,
              MIN(captured_at_ms) AS oldest_captured_at_ms,
              MAX(captured_at_ms) AS newest_captured_at_ms
            FROM metric_samples
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(StoreStats {
            sample_count: row.try_get::<i64, _>("sample_count")?,
            oldest_captured_at_ms: row.try_get::<Option<i64>, _>("oldest_captured_at_ms")?,
            newest_captured_at_ms: row.try_get::<Option<i64>, _>("newest_captured_at_ms")?,
        })
    }

    pub async fn history_coverage(
        &self,
        settings: &DashboardSettings,
    ) -> Result<HistoryCoverage, StoreError> {
        settings.validate()?;
        let stats = self.stats().await?;
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) AS rollup_bucket_count
            FROM metric_rollups_1m
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(HistoryCoverage {
            sample_count: stats.sample_count,
            oldest_captured_at_ms: stats.oldest_captured_at_ms,
            newest_captured_at_ms: stats.newest_captured_at_ms,
            retention_hours: settings.retention_hours,
            rollup_retention_days: settings.rollup_retention_days,
            rollup_bucket_count: row.try_get::<i64, _>("rollup_bucket_count")?,
            database_bytes: self.database_bytes().await?,
        })
    }

    pub async fn prune_raw_history(&self, cutoff_ms: i64) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM metric_samples
            WHERE captured_at_ms < ?
            "#,
        )
        .bind(cutoff_ms)
        .execute(&self.pool)
        .await?;

        self.rebuild_rollup_bucket(bucket_start_ms(cutoff_ms))
            .await?;

        Ok(result.rows_affected())
    }

    pub async fn prune_rollups(&self, cutoff_ms: i64) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM metric_rollups_1m
            WHERE bucket_start_ms < ?
            "#,
        )
        .bind(bucket_start_ms(cutoff_ms))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn integrity_check(&self) -> Result<String, StoreError> {
        let row = sqlx::query("PRAGMA integrity_check")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get::<String, _>(0)?)
    }

    pub async fn vacuum(&self) -> Result<(), StoreError> {
        sqlx::query("VACUUM").execute(&self.pool).await?;
        Ok(())
    }

    async fn database_bytes(&self) -> Result<i64, StoreError> {
        let page_count = sqlx::query("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>(0)?;
        let page_size = sqlx::query("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>(0)?;
        Ok(page_count.saturating_mul(page_size))
    }

    async fn rebuild_rollup_bucket(&self, bucket_start_ms: i64) -> Result<(), StoreError> {
        let bucket_end_ms = bucket_start_ms.saturating_add(60_000);
        let row = sqlx::query(
            r#"
            SELECT
              COUNT(*) AS sample_count,
              MIN(captured_at_ms) AS first_captured_at_ms,
              MAX(captured_at_ms) AS newest_captured_at_ms,
              AVG(cpu_usage_percent) AS avg_cpu_usage_percent,
              MAX(cpu_usage_percent) AS max_cpu_usage_percent,
              AVG(memory_used_percent) AS avg_memory_used_percent,
              MAX(memory_used_percent) AS max_memory_used_percent,
              AVG(swap_used_percent) AS avg_swap_used_percent,
              MAX(swap_used_percent) AS max_swap_used_percent,
              AVG(load_percent) AS avg_load_percent,
              MAX(load_percent) AS max_load_percent,
              AVG(root_used_percent) AS avg_root_used_percent
            FROM metric_samples
            WHERE captured_at_ms >= ? AND captured_at_ms < ?
            "#,
        )
        .bind(bucket_start_ms)
        .bind(bucket_end_ms)
        .fetch_one(&self.pool)
        .await?;

        let sample_count = row.try_get::<i64, _>("sample_count")?;
        if sample_count == 0 {
            sqlx::query(
                r#"
                DELETE FROM metric_rollups_1m
                WHERE bucket_start_ms = ?
                "#,
            )
            .bind(bucket_start_ms)
            .execute(&self.pool)
            .await?;
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO metric_rollups_1m (
              bucket_start_ms,
              first_captured_at_ms,
              newest_captured_at_ms,
              sample_count,
              avg_cpu_usage_percent,
              max_cpu_usage_percent,
              avg_memory_used_percent,
              max_memory_used_percent,
              avg_swap_used_percent,
              max_swap_used_percent,
              avg_load_percent,
              max_load_percent,
              avg_root_used_percent
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_start_ms) DO UPDATE SET
              first_captured_at_ms = excluded.first_captured_at_ms,
              newest_captured_at_ms = excluded.newest_captured_at_ms,
              sample_count = excluded.sample_count,
              avg_cpu_usage_percent = excluded.avg_cpu_usage_percent,
              max_cpu_usage_percent = excluded.max_cpu_usage_percent,
              avg_memory_used_percent = excluded.avg_memory_used_percent,
              max_memory_used_percent = excluded.max_memory_used_percent,
              avg_swap_used_percent = excluded.avg_swap_used_percent,
              max_swap_used_percent = excluded.max_swap_used_percent,
              avg_load_percent = excluded.avg_load_percent,
              max_load_percent = excluded.max_load_percent,
              avg_root_used_percent = excluded.avg_root_used_percent
            "#,
        )
        .bind(bucket_start_ms)
        .bind(row.try_get::<i64, _>("first_captured_at_ms")?)
        .bind(row.try_get::<i64, _>("newest_captured_at_ms")?)
        .bind(sample_count)
        .bind(row.try_get::<f64, _>("avg_cpu_usage_percent")?)
        .bind(row.try_get::<f64, _>("max_cpu_usage_percent")?)
        .bind(row.try_get::<f64, _>("avg_memory_used_percent")?)
        .bind(row.try_get::<f64, _>("max_memory_used_percent")?)
        .bind(row.try_get::<f64, _>("avg_swap_used_percent")?)
        .bind(row.try_get::<f64, _>("max_swap_used_percent")?)
        .bind(row.try_get::<f64, _>("avg_load_percent")?)
        .bind(row.try_get::<f64, _>("max_load_percent")?)
        .bind(row.try_get::<Option<f64>, _>("avg_root_used_percent")?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn apply_schema(&self) -> Result<(), StoreError> {
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA busy_timeout = 5000")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
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
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
              ON metric_samples (captured_at_ms DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
              ON metric_samples (runtime_kind, captured_at_ms DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS app_settings (
              setting_key TEXT PRIMARY KEY,
              value_json TEXT NOT NULL,
              updated_at_ms INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
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
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_metric_rollups_1m_newest
              ON metric_rollups_1m (newest_captured_at_ms DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum StoreError {
    Sqlx(sqlx::Error),
    Json(serde_json::Error),
    IntegerOverflow { field: &'static str },
    Validation(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::IntegerOverflow { field } => {
                write!(formatter, "{field} does not fit in SQLite INTEGER")
            }
            Self::Validation(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::IntegerOverflow { .. } | Self::Validation(_) => None,
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

fn row_to_sample(row: sqlx::sqlite::SqliteRow) -> Result<HistorySample, StoreError> {
    let captured_at_ms = row.try_get::<i64, _>("captured_at_ms")?;
    let snapshot_json = row.try_get::<String, _>("snapshot_json")?;
    let snapshot = serde_json::from_str(&snapshot_json)?;
    Ok(HistorySample {
        captured_at_ms,
        snapshot,
    })
}

fn to_i64(value: impl TryInto<i64>, field: &'static str) -> Result<i64, StoreError> {
    value
        .try_into()
        .map_err(|_| StoreError::IntegerOverflow { field })
}

fn now_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.min(i64::MAX as u128) as i64
}

fn validate_one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), StoreError> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(StoreError::Validation(format!(
        "{field} must be one of {}",
        allowed.join(", ")
    )))
}

fn validate_range(field: &str, value: i64, min: i64, max: i64) -> Result<(), StoreError> {
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(StoreError::Validation(format!(
        "{field} must be between {min} and {max}"
    )))
}

fn validate_threshold_pair(field: &str, warn: i64, critical: i64) -> Result<(), StoreError> {
    if warn <= critical {
        return Ok(());
    }
    Err(StoreError::Validation(format!(
        "{field} warning threshold must be less than or equal to critical threshold"
    )))
}

fn load_percent(snapshot: &SystemSnapshot) -> f64 {
    if snapshot.cpu.cores == 0 {
        0.0
    } else {
        ((snapshot.load.one / snapshot.cpu.cores as f64) * 100.0).clamp(0.0, 100.0)
    }
}

fn bucket_start_ms(captured_at_ms: i64) -> i64 {
    captured_at_ms.div_euclid(60_000).saturating_mul(60_000)
}

fn default_cpu_critical() -> i64 {
    95
}

fn default_memory_critical() -> i64 {
    95
}

fn default_disk_critical() -> i64 {
    95
}

fn default_load_warn() -> i64 {
    80
}

fn default_load_critical() -> i64 {
    100
}

fn default_pressure_warn() -> i64 {
    10
}

fn default_pressure_critical() -> i64 {
    25
}
