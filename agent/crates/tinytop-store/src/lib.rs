pub mod archive;
pub mod disk;
pub mod ladder;
pub mod maintenance;
pub mod migration;
pub mod otel_settings;
pub mod retention_ladder;
pub mod settings_transfer;

use std::{
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value as JsonValue;
use sqlx::{
    AssertSqlSafe, Row, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
pub use tinytop_types::SystemSnapshot;

pub use crate::disk::{
    DiskCheckReport, DiskTransition, FreeBytesProvider, SysinfoFreeBytes, apply_disk_measurement,
    check_disk,
};
use crate::ladder::{RawSampleRow, Stat, Tier, TierBucket, fold, raw_is_partial, raw_to_bucket};
pub use crate::migration::pre_image_path;
use crate::otel_settings::OtelSettings;
pub use crate::retention_ladder::DiskPressureState;
use crate::retention_ladder::RetentionLadder;

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
    #[serde(default = "RetentionLadder::default_for_serde")]
    pub retention_ladder: RetentionLadder,
    #[serde(default = "OtelSettings::default_for_serde")]
    pub otel: OtelSettings,
    #[serde(default = "default_target_database_bytes")]
    pub target_database_bytes: i64,
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
            retention_ladder: RetentionLadder::default(),
            otel: OtelSettings::default(),
            target_database_bytes: default_target_database_bytes(),
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
    pub fn from_document(
        document: JsonValue,
        persisted: Option<&DashboardSettings>,
    ) -> Result<Self, StoreError> {
        let has_retention_ladder = document.get("retentionLadder").is_some();
        let has_otel = document.get("otel").is_some();
        let mut settings: Self = serde_json::from_value(document).map_err(|error| {
            StoreError::Validation(format!("settings document could not be decoded: {error}"))
        })?;
        if !has_retention_ladder {
            settings.retention_ladder = match persisted {
                Some(persisted) => {
                    let mut ladder = persisted.retention_ladder.clone();
                    ladder.apply_legacy_aliases(
                        settings.retention_hours,
                        settings.rollup_retention_days,
                    );
                    ladder
                }
                None => RetentionLadder::from_legacy(
                    settings.retention_hours,
                    settings.rollup_retention_days,
                ),
            };
        }
        if !has_otel {
            settings.otel = persisted
                .map(|value| value.otel.clone())
                .unwrap_or_default();
        }
        Ok(settings)
    }

    pub fn normalize_legacy_mirrors(&mut self) {
        self.retention_hours = self.retention_ladder.l1.keep_days.saturating_mul(24);
        self.rollup_retention_days = self.retention_ladder.l2.keep_days;
    }

    pub fn changed_keys(previous: &Self, next: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if previous.default_theme != next.default_theme {
            changed.push("defaultTheme");
        }
        if previous.default_graph_mode != next.default_graph_mode {
            changed.push("defaultGraphMode");
        }
        if previous.poll_interval_ms != next.poll_interval_ms {
            changed.push("pollIntervalMs");
        }
        if previous.default_history_window != next.default_history_window {
            changed.push("defaultHistoryWindow");
        }
        if previous.retention_ladder != next.retention_ladder {
            changed.push("retentionLadder");
        }
        if previous.otel != next.otel {
            changed.push("otel");
        }
        if previous.target_database_bytes != next.target_database_bytes {
            changed.push("targetDatabaseBytes");
        }
        if previous.top_process_count != next.top_process_count {
            changed.push("topProcessCount");
        }
        if previous.redaction_default != next.redaction_default {
            changed.push("redactionDefault");
        }
        if previous.thresholds != next.thresholds {
            changed.push("thresholds");
        }
        if previous.enabled_sections != next.enabled_sections {
            changed.push("enabledSections");
        }
        changed
    }

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
            &[
                "live", "15m", "1h", "6h", "24h", "7d", "30d", "90d", "1y", "all",
            ],
        )?;
        validate_range("pollIntervalMs", self.poll_interval_ms, 250, 60_000)?;
        validate_range(
            "targetDatabaseBytes",
            self.target_database_bytes,
            1_048_576,
            10_737_418_240,
        )?;
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
        self.retention_ladder.validate(None, None)?;
        self.otel.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCoverage {
    pub sample_count: i64,
    pub oldest_captured_at_ms: Option<i64>,
    pub newest_captured_at_ms: Option<i64>,
    pub retention_hours: i64,
    pub rollup_retention_days: i64,
    pub rollup_bucket_count: i64,
    pub database_bytes: i64,
    pub target_database_bytes: i64,
    pub database_budget_percent: f64,
    pub rollup_oldest_captured_at_ms: Option<i64>,
    pub rollup_newest_captured_at_ms: Option<i64>,
    pub tiers: Vec<HistoryTierCoverage>,
    pub snapshot_json_oldest_ms: Option<i64>,
    #[serde(default, skip_serializing)]
    pub snapshot_json_sample_count: i64,
    pub detail_interval_sec: i64,
    pub disk: HistoryDiskCoverage,
    pub archive: HistoryArchiveCoverage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otel: Option<HistoryOtelCoverage>,
    pub migration: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryOtelCoverage {
    pub enabled: bool,
    pub endpoint: String,
    pub interval_sec: i64,
    pub last_success_ms: Option<i64>,
    pub last_failure_ms: Option<i64>,
    pub failures: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTierCoverage {
    pub tier: String,
    pub enabled: bool,
    pub keep_days: i64,
    pub resolution_ms: i64,
    pub bucket_count: i64,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDiskCoverage {
    pub free_bytes: Option<i64>,
    pub min_free_bytes: i64,
    pub pressure: bool,
    pub pressure_since_ms: Option<i64>,
    pub last_check_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryArchiveCoverage {
    pub queryable: HistoryQueryableArchiveCoverage,
    pub cold: HistoryColdArchiveCoverage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQueryableArchiveCoverage {
    pub enabled: bool,
    pub path: String,
    pub bucket_count: i64,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryColdArchiveCoverage {
    pub enabled: bool,
    pub directory: String,
    pub exported_until_month: Option<String>,
    pub file_count: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFilesystemSample {
    pub captured_at_ms: i64,
    pub mount: String,
    pub filesystem: String,
    #[serde(rename = "type")]
    pub fs_type: String,
    pub size_bytes: i64,
    pub used_bytes: i64,
    pub available_bytes: i64,
    pub used_percent: f64,
    pub inode_used_percent: Option<f64>,
    pub inode_used: Option<i64>,
    pub inode_total: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryProcessSample {
    pub rank: i64,
    pub pid: i64,
    pub command: String,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub rss_bytes: i64,
    pub parent_pid: Option<i64>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryProcessCapture {
    pub captured_at_ms: i64,
    pub processes: Vec<HistoryProcessSample>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryQuery {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryPointSource {
    Raw,
    Rollup,
    #[serde(rename = "5m")]
    Rollup5m,
    #[serde(rename = "1h")]
    Rollup1h,
    Archive,
}

impl HistoryPointSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Rollup => "rollup",
            Self::Rollup5m => "5m",
            Self::Rollup1h => "1h",
            Self::Archive => "archive",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HistoryPointMode {
    #[default]
    Auto,
    Raw,
    Rollup,
    Rollup5m,
    Rollup1h,
    Archive,
}

impl HistoryPointMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Raw => "raw",
            Self::Rollup => "rollup",
            Self::Rollup5m => "5m",
            Self::Rollup1h => "1h",
            Self::Archive => "archive",
        }
    }

    pub fn resolution_ms(self, poll_interval_ms: i64) -> i64 {
        match self {
            Self::Auto | Self::Raw => poll_interval_ms,
            Self::Rollup => Tier::L2.resolution_ms(),
            Self::Rollup5m => Tier::L3.resolution_ms(),
            Self::Rollup1h | Self::Archive => Tier::L4.resolution_ms(),
        }
    }
}

impl FromStr for HistoryPointMode {
    type Err = StoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "raw" => Ok(Self::Raw),
            "rollup" | "1m" | "rollup1m" => Ok(Self::Rollup),
            "5m" => Ok(Self::Rollup5m),
            "1h" => Ok(Self::Rollup1h),
            "archive" => Ok(Self::Archive),
            other => Err(StoreError::Validation(format!(
                "source must be auto, raw, rollup, 5m, 1h, or archive, got {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryPointsQuery {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: Option<i64>,
    pub source: HistoryPointMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub captured_at_ms: i64,
    pub source: HistoryPointSource,
    pub sample_count: i64,
    pub cpu_usage_percent: f64,
    pub memory_used_percent: f64,
    pub swap_used_percent: f64,
    pub load_percent: f64,
    pub root_used_percent: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryMarkerType {
    DaemonStart,
    SettingsChange,
    CoverageGap,
    SchemaMigrated,
    DiskPressure,
    DiskRecovered,
}

impl HistoryMarkerType {
    fn as_str(self) -> &'static str {
        match self {
            Self::DaemonStart => "daemonStart",
            Self::SettingsChange => "settingsChange",
            Self::CoverageGap => "coverageGap",
            Self::SchemaMigrated => "schemaMigrated",
            Self::DiskPressure => "diskPressure",
            Self::DiskRecovered => "diskRecovered",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "daemonStart" => Ok(Self::DaemonStart),
            "settingsChange" => Ok(Self::SettingsChange),
            "coverageGap" => Ok(Self::CoverageGap),
            "schemaMigrated" => Ok(Self::SchemaMigrated),
            "diskPressure" => Ok(Self::DiskPressure),
            "diskRecovered" => Ok(Self::DiskRecovered),
            other => Err(StoreError::Validation(format!(
                "unknown history marker type {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMarker {
    pub occurred_at_ms: i64,
    pub marker_type: HistoryMarkerType,
    pub label: String,
    pub details: JsonValue,
}

#[derive(Debug, Clone)]
pub struct SqliteHistoryStore {
    pool: SqlitePool,
    database_path: PathBuf,
}

pub(crate) async fn history_state_get_on<T: DeserializeOwned>(
    connection: &mut SqliteConnection,
    key: &str,
) -> Result<Option<T>, StoreError> {
    let value_json: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM history_state WHERE state_key = ?")
            .bind(key)
            .fetch_optional(&mut *connection)
            .await?;
    value_json
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(StoreError::from)
}

async fn get_settings_on(
    connection: &mut SqliteConnection,
) -> Result<DashboardSettings, StoreError> {
    let row = sqlx::query(
        r#"
        SELECT value_json
        FROM app_settings
        WHERE setting_key = 'dashboard'
        "#,
    )
    .fetch_optional(&mut *connection)
    .await?;

    let Some(row) = row else {
        return Ok(DashboardSettings::default());
    };

    let value_json = row.try_get::<String, _>("value_json")?;
    let document: JsonValue = serde_json::from_str(&value_json)?;
    let settings = DashboardSettings::from_document(document, None)?;
    settings.validate()?;
    Ok(settings)
}

async fn rollback_settings_transaction<T>(
    connection: &mut SqliteConnection,
    error: StoreError,
) -> Result<T, StoreError> {
    if let Err(source) = sqlx::query("ROLLBACK").execute(&mut *connection).await {
        eprintln!("settings rollback failed after {error}: {source}");
    }
    Err(error)
}

pub(crate) async fn history_state_set_on<T: Serialize>(
    connection: &mut SqliteConnection,
    key: &str,
    value: &T,
    now_ms: i64,
) -> Result<(), StoreError> {
    let value_json = serde_json::to_string(value)?;
    sqlx::query(
        r#"
        INSERT INTO history_state (state_key, value_json, updated_at_ms)
        VALUES (?, ?, ?)
        ON CONFLICT(state_key) DO UPDATE SET
          value_json = excluded.value_json,
          updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(key)
    .bind(value_json)
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn record_event_on(
    connection: &mut SqliteConnection,
    occurred_at_ms: i64,
    marker_type: HistoryMarkerType,
    label: &str,
    details: JsonValue,
) -> Result<(), StoreError> {
    let details_json = serde_json::to_string(&details)?;
    sqlx::query(
        r#"
        INSERT INTO app_events (
          occurred_at_ms,
          marker_type,
          label,
          details_json
        ) VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(occurred_at_ms)
    .bind(marker_type.as_str())
    .bind(label)
    .bind(details_json)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

impl SqliteHistoryStore {
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
        let db_path = options.get_filename().to_path_buf();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let store = Self {
            pool,
            database_path: db_path.clone(),
        };
        store.apply_pragmas().await?;
        let _migration_report = migration::ensure_schema(
            &store.pool,
            &db_path,
            now_ms(),
            migration::DEFAULT_SNAPSHOT_JSON_KEEP_MS,
        )
        .await?;
        store.migrate_runtime_kind_to_canonical().await?;
        Ok(store)
    }

    /// Open an existing database for CLI inspection without running schema migration.
    ///
    /// Inspection never creates a missing database. Existing databases are
    /// opened as-is so pre-image removal checks can observe a pre-migration
    /// `user_version` instead of changing it first.
    pub async fn connect_for_inspection(database_url: &str) -> Result<Self, StoreError> {
        let database_path = match inspect_database_path(database_url)? {
            Some(database_path) => database_path,
            None => {
                let configured_path = database_path_from_url(database_url)?;
                return Err(StoreError::Migration {
                    reason: format!(
                        "cannot inspect database {} because it does not exist",
                        configured_path.display()
                    ),
                    remedy: "restore or recreate the database before inspecting it".to_string(),
                });
            }
        };
        let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(false);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Ok(Self {
            pool,
            database_path,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub async fn close(self) -> Result<(), StoreError> {
        self.pool.close().await;
        Ok(())
    }

    pub async fn user_version(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&self.pool)
            .await?)
    }

    #[doc(hidden)]
    pub async fn attached_database_names(&self) -> Result<Vec<String>, StoreError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("PRAGMA database_list")
            .fetch_all(&mut *connection)
            .await?
            .into_iter()
            .map(|row| row.try_get("name").map_err(StoreError::from))
            .collect()
    }

    pub async fn get_settings(&self) -> Result<DashboardSettings, StoreError> {
        let mut connection = self.pool.acquire().await?;
        get_settings_on(&mut connection).await
    }

    pub async fn history_state_get<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, StoreError> {
        let mut connection = self.pool.acquire().await?;
        history_state_get_on(&mut connection, key).await
    }

    pub async fn history_state_set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let mut connection = self.pool.acquire().await?;
        history_state_set_on(&mut connection, key, value, now_ms).await
    }

    pub async fn put_settings(
        &self,
        settings: &DashboardSettings,
    ) -> Result<DashboardSettings, StoreError> {
        let mut normalized = settings.clone();
        normalized.normalize_legacy_mirrors();
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;

        let transaction_result = async {
            let previous = get_settings_on(&mut connection).await?;
            let disk_pressure =
                history_state_get_on::<DiskPressureState>(&mut connection, "diskPressure")
                    .await?
                    .unwrap_or_default();
            normalized.validate()?;
            normalized
                .retention_ladder
                .validate(Some(&disk_pressure), Some(&previous.retention_ladder))?;

            let value_json = serde_json::to_string(&normalized)?;
            let updated_at_ms = now_ms();
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
            .bind(updated_at_ms)
            .execute(&mut *connection)
            .await?;

            for (key, enabled) in [
                ("l3Enabled", normalized.retention_ladder.l3.enabled),
                ("l4Enabled", normalized.retention_ladder.l4.enabled),
            ] {
                let enabled_json = serde_json::to_string(&enabled)?;
                sqlx::query(
                    r#"
                    INSERT INTO history_state (state_key, value_json, updated_at_ms)
                    VALUES (?, ?, ?)
                    ON CONFLICT(state_key) DO UPDATE SET
                      value_json = excluded.value_json,
                      updated_at_ms = excluded.updated_at_ms
                    "#,
                )
                .bind(key)
                .bind(enabled_json)
                .bind(updated_at_ms)
                .execute(&mut *connection)
                .await?;
            }

            Ok::<DashboardSettings, StoreError>(normalized)
        }
        .await;
        let normalized = match transaction_result {
            Ok(normalized) => normalized,
            Err(error) => return rollback_settings_transaction(&mut connection, error).await,
        };
        if let Err(source) = sqlx::query("COMMIT").execute(&mut *connection).await {
            return rollback_settings_transaction(&mut connection, StoreError::from(source)).await;
        }

        Ok(normalized)
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
        let new_sample = raw_to_bucket(&RawSampleRow {
            captured_at_ms,
            cpu_usage_percent: snapshot.cpu.usage_percent,
            memory_used_percent: snapshot.memory.used_percent,
            swap_used_percent: snapshot.swap.used_percent,
            load_percent: load_percent(snapshot),
            root_used_percent,
        });
        let raw_row_existed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM metric_samples WHERE captured_at_ms = ?)",
        )
        .bind(captured_at_ms)
        .fetch_one(&self.pool)
        .await?;

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
        .bind(snapshot.identity.runtime.kind.as_str())
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

        let settings = self.get_settings().await?;
        let detail_interval_ms = settings
            .retention_ladder
            .detail_interval_sec
            .saturating_mul(1_000);
        self.write_detail_rows_if_due(captured_at_ms, snapshot, detail_interval_ms)
            .await?;
        let minute_start_ms = bucket_start_ms(captured_at_ms);
        let minute_end_ms = minute_start_ms.saturating_add(Tier::L2.resolution_ms());
        let existing_bucket = self
            .read_tier_buckets(Tier::L2, minute_start_ms, minute_end_ms)
            .await?
            .into_iter()
            .next();
        let raw_count_now = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM metric_samples
            WHERE captured_at_ms >= ? AND captured_at_ms < ?
            "#,
        )
        .bind(minute_start_ms)
        .bind(minute_end_ms)
        .fetch_one(&self.pool)
        .await?;

        let l2_changed = if let Some(existing_bucket) =
            existing_bucket.filter(|bucket| raw_is_partial(bucket.sample_count, raw_count_now))
        {
            if raw_row_existed {
                // Replay limit: after pruning, the first replay of a represented timestamp is
                // indistinguishable from a late write and adds one count; later replays must not
                // merge that already-counted raw row into the frozen/partial bucket again.
                false
            } else {
                let Some(bucket) = fold(minute_start_ms, &[existing_bucket, new_sample.clone()])
                else {
                    return Err(StoreError::Validation(
                        "an existing minute bucket and raw sample must have a positive sample count"
                            .to_string(),
                    ));
                };
                self.upsert_tier_bucket(Tier::L2, &bucket).await?;
                true
            }
        } else {
            self.rebuild_rollup_bucket(minute_start_ms).await?;
            true
        };
        if l2_changed {
            maintenance::refold_ancestors_for_late_write(
                self,
                captured_at_ms,
                now_ms(),
                &settings.retention_ladder,
                &new_sample,
                !raw_row_existed,
            )
            .await?;
        }

        Ok(HistorySample {
            captured_at_ms,
            snapshot: snapshot.clone(),
        })
    }

    pub async fn read_tier_buckets(
        &self,
        tier: Tier,
        since_ms: i64,
        until_ms: i64,
    ) -> Result<Vec<TierBucket>, StoreError> {
        if tier == Tier::L1 {
            let rows = sqlx::query(
                r#"
                SELECT captured_at_ms, cpu_usage_percent, memory_used_percent,
                       swap_used_percent, load_percent, root_used_percent
                FROM metric_samples
                WHERE captured_at_ms >= ? AND captured_at_ms < ?
                ORDER BY captured_at_ms
                "#,
            )
            .bind(since_ms)
            .bind(until_ms)
            .fetch_all(&self.pool)
            .await?;
            return rows
                .into_iter()
                .map(|row| {
                    Ok(raw_to_bucket(&RawSampleRow {
                        captured_at_ms: row.try_get("captured_at_ms")?,
                        cpu_usage_percent: row.try_get("cpu_usage_percent")?,
                        memory_used_percent: row.try_get("memory_used_percent")?,
                        swap_used_percent: row.try_get("swap_used_percent")?,
                        load_percent: row.try_get("load_percent")?,
                        root_used_percent: row.try_get("root_used_percent")?,
                    }))
                })
                .collect();
        }

        let sql = format!(
            r#"
            SELECT bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
                   avg_cpu_usage_percent,
                   COALESCE(min_cpu_usage_percent, avg_cpu_usage_percent) AS min_cpu_usage_percent,
                   max_cpu_usage_percent,
                   avg_memory_used_percent,
                   COALESCE(min_memory_used_percent, avg_memory_used_percent) AS min_memory_used_percent,
                   max_memory_used_percent,
                   avg_swap_used_percent,
                   COALESCE(min_swap_used_percent, avg_swap_used_percent) AS min_swap_used_percent,
                   max_swap_used_percent,
                   avg_load_percent,
                   COALESCE(min_load_percent, avg_load_percent) AS min_load_percent,
                   max_load_percent,
                   avg_root_used_percent,
                   COALESCE(min_root_used_percent, avg_root_used_percent) AS min_root_used_percent,
                   COALESCE(max_root_used_percent, avg_root_used_percent) AS max_root_used_percent
            FROM {}
            WHERE bucket_start_ms >= ? AND bucket_start_ms < ?
            ORDER BY bucket_start_ms
            "#,
            tier.table()
        );
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(since_ms)
            .bind(until_ms)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(tier_bucket_from_row).collect()
    }

    pub async fn upsert_tier_bucket(
        &self,
        tier: Tier,
        bucket: &TierBucket,
    ) -> Result<(), StoreError> {
        if tier == Tier::L1 {
            return Err(StoreError::Validation(
                "Tier::L1 is written through insert_snapshot, not upsert_tier_bucket".to_string(),
            ));
        }
        let sql = format!(
            r#"
            INSERT INTO {} (
              bucket_start_ms, first_captured_at_ms, newest_captured_at_ms, sample_count,
              avg_cpu_usage_percent, min_cpu_usage_percent, max_cpu_usage_percent,
              avg_memory_used_percent, min_memory_used_percent, max_memory_used_percent,
              avg_swap_used_percent, min_swap_used_percent, max_swap_used_percent,
              avg_load_percent, min_load_percent, max_load_percent,
              avg_root_used_percent, min_root_used_percent, max_root_used_percent
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_start_ms) DO UPDATE SET
              first_captured_at_ms = excluded.first_captured_at_ms,
              newest_captured_at_ms = excluded.newest_captured_at_ms,
              sample_count = excluded.sample_count,
              avg_cpu_usage_percent = excluded.avg_cpu_usage_percent,
              min_cpu_usage_percent = excluded.min_cpu_usage_percent,
              max_cpu_usage_percent = excluded.max_cpu_usage_percent,
              avg_memory_used_percent = excluded.avg_memory_used_percent,
              min_memory_used_percent = excluded.min_memory_used_percent,
              max_memory_used_percent = excluded.max_memory_used_percent,
              avg_swap_used_percent = excluded.avg_swap_used_percent,
              min_swap_used_percent = excluded.min_swap_used_percent,
              max_swap_used_percent = excluded.max_swap_used_percent,
              avg_load_percent = excluded.avg_load_percent,
              min_load_percent = excluded.min_load_percent,
              max_load_percent = excluded.max_load_percent,
              avg_root_used_percent = excluded.avg_root_used_percent,
              min_root_used_percent = excluded.min_root_used_percent,
              max_root_used_percent = excluded.max_root_used_percent
            "#,
            tier.table()
        );
        sqlx::query(AssertSqlSafe(sql))
            .bind(bucket.bucket_start_ms)
            .bind(bucket.first_captured_at_ms)
            .bind(bucket.newest_captured_at_ms)
            .bind(bucket.sample_count)
            .bind(bucket.cpu.avg)
            .bind(bucket.cpu.min)
            .bind(bucket.cpu.max)
            .bind(bucket.memory.avg)
            .bind(bucket.memory.min)
            .bind(bucket.memory.max)
            .bind(bucket.swap.avg)
            .bind(bucket.swap.min)
            .bind(bucket.swap.max)
            .bind(bucket.load.avg)
            .bind(bucket.load.min)
            .bind(bucket.load.max)
            .bind(bucket.root_used.map(|stat| stat.avg))
            .bind(bucket.root_used.map(|stat| stat.min))
            .bind(bucket.root_used.map(|stat| stat.max))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn oldest_tier_bucket_start_at_or_after(
        &self,
        tier: Tier,
        since_ms: i64,
    ) -> Result<Option<i64>, StoreError> {
        let time_column = if tier == Tier::L1 {
            "captured_at_ms"
        } else {
            "bucket_start_ms"
        };
        let sql = format!(
            "SELECT MIN({time_column}) FROM {} WHERE {time_column} >= ?",
            tier.table()
        );
        Ok(sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(since_ms)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn write_detail_rows_if_due(
        &self,
        captured_at_ms: i64,
        snapshot: &SystemSnapshot,
        interval_ms: i64,
    ) -> Result<i64, StoreError> {
        let last_detail_ms = self.history_state_get::<i64>("lastDetailMs").await?;
        if last_detail_ms.is_some_and(|last| {
            captured_at_ms != last && captured_at_ms.saturating_sub(last) < interval_ms
        }) {
            return Ok(0);
        }
        let detail_rows = to_i64(
            snapshot
                .filesystems
                .len()
                .saturating_add(snapshot.processes.len()),
            "detail row count",
        )?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM fs_samples WHERE captured_at_ms = ?")
            .bind(captured_at_ms)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM process_samples WHERE captured_at_ms = ?")
            .bind(captured_at_ms)
            .execute(&mut *transaction)
            .await?;
        for filesystem in &snapshot.filesystems {
            sqlx::query(
                r#"
                INSERT INTO fs_samples (
                  captured_at_ms, mount, filesystem, fs_type, size_bytes, used_bytes,
                  available_bytes, used_percent, inode_used_percent, inode_used, inode_total
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(captured_at_ms, mount) DO UPDATE SET
                  filesystem = excluded.filesystem, fs_type = excluded.fs_type,
                  size_bytes = excluded.size_bytes, used_bytes = excluded.used_bytes,
                  available_bytes = excluded.available_bytes, used_percent = excluded.used_percent,
                  inode_used_percent = excluded.inode_used_percent,
                  inode_used = excluded.inode_used, inode_total = excluded.inode_total
                "#,
            )
            .bind(captured_at_ms)
            .bind(&filesystem.mount)
            .bind(&filesystem.filesystem)
            .bind(&filesystem.fs_type)
            .bind(to_i64(filesystem.size_bytes, "filesystem size bytes")?)
            .bind(to_i64(filesystem.used_bytes, "filesystem used bytes")?)
            .bind(to_i64(
                filesystem.available_bytes,
                "filesystem available bytes",
            )?)
            .bind(filesystem.used_percent)
            .bind(filesystem.inode_used_percent)
            .bind(
                filesystem
                    .inode_used
                    .map(|value| to_i64(value, "filesystem inode used"))
                    .transpose()?,
            )
            .bind(
                filesystem
                    .inode_total
                    .map(|value| to_i64(value, "filesystem inode total"))
                    .transpose()?,
            )
            .execute(&mut *transaction)
            .await?;
        }
        for (rank, process) in snapshot.processes.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO process_samples (
                  captured_at_ms, rank, pid, command, cpu_percent, memory_percent,
                  rss_bytes, parent_pid, started_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(captured_at_ms, rank) DO UPDATE SET
                  pid = excluded.pid, command = excluded.command,
                  cpu_percent = excluded.cpu_percent, memory_percent = excluded.memory_percent,
                  rss_bytes = excluded.rss_bytes, parent_pid = excluded.parent_pid,
                  started_at = excluded.started_at
                "#,
            )
            .bind(captured_at_ms)
            .bind(to_i64(rank, "process rank")?)
            .bind(to_i64(process.pid, "process pid")?)
            .bind(&process.command)
            .bind(process.cpu_percent)
            .bind(process.memory_percent)
            .bind(to_i64(process.rss_bytes, "process rss bytes")?)
            .bind(
                process
                    .parent_pid
                    .map(|value| to_i64(value, "process parent pid"))
                    .transpose()?,
            )
            .bind(&process.started_at)
            .execute(&mut *transaction)
            .await?;
        }
        let value_json = serde_json::to_string(&captured_at_ms)?;
        sqlx::query(
            r#"
            INSERT INTO history_state (state_key, value_json, updated_at_ms)
            VALUES ('lastDetailMs', ?, ?)
            ON CONFLICT(state_key) DO UPDATE SET
              value_json = excluded.value_json, updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(value_json)
        .bind(captured_at_ms)
        .execute(&mut *transaction)
        .await?;
        let detail_rows_json = serde_json::to_string(&detail_rows)?;
        sqlx::query(
            r#"
            INSERT INTO history_state (state_key, value_json, updated_at_ms)
            VALUES ('pendingDetailRows', ?, ?)
            ON CONFLICT(state_key) DO UPDATE SET
              value_json = excluded.value_json, updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(detail_rows_json)
        .bind(captured_at_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(detail_rows)
    }

    pub async fn latest_snapshot(&self) -> Result<Option<HistorySample>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT captured_at_ms, snapshot_json
            FROM metric_samples
            WHERE snapshot_json IS NOT NULL
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
            WHERE snapshot_json IS NOT NULL
              AND (?1 IS NULL OR captured_at_ms >= ?1)
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

    pub async fn read_history_points(
        &self,
        query: HistoryPointsQuery,
    ) -> Result<Vec<HistoryPoint>, StoreError> {
        let settings = if matches!(
            query.source,
            HistoryPointMode::Auto | HistoryPointMode::Archive
        ) {
            Some(self.get_settings().await?)
        } else {
            None
        };
        let source = match query.source {
            HistoryPointMode::Auto => {
                let settings = settings
                    .as_ref()
                    .expect("auto history point reads fetch settings above");
                resolve_history_point_source_with_poll(
                    &settings.retention_ladder,
                    settings.poll_interval_ms,
                    now_ms(),
                    query,
                )
            }
            source => source,
        };
        match source {
            HistoryPointMode::Raw => self.read_tier_history_points(Tier::L1, query).await,
            HistoryPointMode::Rollup => self.read_tier_history_points(Tier::L2, query).await,
            HistoryPointMode::Rollup5m => self.read_tier_history_points(Tier::L3, query).await,
            HistoryPointMode::Rollup1h => self.read_tier_history_points(Tier::L4, query).await,
            HistoryPointMode::Archive => {
                let settings = settings
                    .as_ref()
                    .expect("archive history point reads fetch settings above");
                if !settings.retention_ladder.archive.queryable {
                    return Ok(Vec::new());
                }
                let paths = archive::archive_paths(
                    self.database_path(),
                    &settings.retention_ladder.archive,
                );
                let buckets = archive::read_archive_points(
                    &paths,
                    query.since_ms.unwrap_or(i64::MIN),
                    query.until_ms.unwrap_or(i64::MAX),
                    effective_points_limit(&query),
                )
                .await?;
                Ok(buckets
                    .into_iter()
                    .map(|bucket| history_point_from_bucket(bucket, HistoryPointSource::Archive))
                    .collect())
            }
            HistoryPointMode::Auto => unreachable!("history point source is resolved above"),
        }
    }

    pub async fn read_history_filesystems(
        &self,
        query: HistoryQuery,
        mount: Option<&str>,
    ) -> Result<Vec<HistoryFilesystemSample>, StoreError> {
        let limit = query.limit.unwrap_or(120).clamp(1, 10_000);
        let rows = sqlx::query(
            r#"
            SELECT captured_at_ms, mount, filesystem, fs_type, size_bytes, used_bytes,
                   available_bytes, used_percent, inode_used_percent, inode_used, inode_total
            FROM fs_samples
            WHERE (?1 IS NULL OR captured_at_ms >= ?1)
              AND (?2 IS NULL OR captured_at_ms <= ?2)
              AND (?3 IS NULL OR mount = ?3)
            ORDER BY captured_at_ms DESC, mount DESC
            LIMIT ?4
            "#,
        )
        .bind(query.since_ms)
        .bind(query.until_ms)
        .bind(mount)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut samples = rows
            .into_iter()
            .map(|row| {
                Ok(HistoryFilesystemSample {
                    captured_at_ms: row.try_get("captured_at_ms")?,
                    mount: row.try_get("mount")?,
                    filesystem: row.try_get("filesystem")?,
                    fs_type: row.try_get("fs_type")?,
                    size_bytes: row.try_get("size_bytes")?,
                    used_bytes: row.try_get("used_bytes")?,
                    available_bytes: row.try_get("available_bytes")?,
                    used_percent: row.try_get("used_percent")?,
                    inode_used_percent: row.try_get("inode_used_percent")?,
                    inode_used: row.try_get("inode_used")?,
                    inode_total: row.try_get("inode_total")?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        samples.reverse();
        Ok(samples)
    }

    pub async fn read_history_processes(
        &self,
        query: HistoryQuery,
    ) -> Result<Vec<HistoryProcessCapture>, StoreError> {
        let limit = query.limit.unwrap_or(120).clamp(1, 10_000);
        let rows = sqlx::query(
            r#"
            WITH capture_times AS (
              SELECT DISTINCT captured_at_ms
              FROM process_samples
              WHERE (?1 IS NULL OR captured_at_ms >= ?1)
                AND (?2 IS NULL OR captured_at_ms <= ?2)
              ORDER BY captured_at_ms DESC
              LIMIT ?3
            )
            SELECT p.captured_at_ms, p.rank, p.pid, p.command, p.cpu_percent,
                   p.memory_percent, p.rss_bytes, p.parent_pid, p.started_at
            FROM process_samples p
            INNER JOIN capture_times c ON c.captured_at_ms = p.captured_at_ms
            ORDER BY p.captured_at_ms, p.rank
            "#,
        )
        .bind(query.since_ms)
        .bind(query.until_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut captures = Vec::<HistoryProcessCapture>::new();
        for row in rows {
            let captured_at_ms: i64 = row.try_get("captured_at_ms")?;
            if captures
                .last()
                .is_none_or(|capture| capture.captured_at_ms != captured_at_ms)
            {
                captures.push(HistoryProcessCapture {
                    captured_at_ms,
                    processes: Vec::new(),
                });
            }
            let process = HistoryProcessSample {
                rank: row.try_get("rank")?,
                pid: row.try_get("pid")?,
                command: row.try_get("command")?,
                cpu_percent: row.try_get("cpu_percent")?,
                memory_percent: row.try_get("memory_percent")?,
                rss_bytes: row.try_get("rss_bytes")?,
                parent_pid: row.try_get("parent_pid")?,
                started_at: row.try_get("started_at")?,
            };
            let Some(capture) = captures.last_mut() else {
                return Err(StoreError::Validation(
                    "process capture grouping produced a row without a timestamp group".to_string(),
                ));
            };
            capture.processes.push(process);
        }
        Ok(captures)
    }

    pub async fn record_event(
        &self,
        occurred_at_ms: i64,
        marker_type: HistoryMarkerType,
        label: &str,
        details: JsonValue,
    ) -> Result<(), StoreError> {
        let mut connection = self.pool.acquire().await?;
        record_event_on(&mut connection, occurred_at_ms, marker_type, label, details).await
    }

    pub async fn read_history_markers(
        &self,
        query: HistoryQuery,
        expected_gap_ms: i64,
    ) -> Result<Vec<HistoryMarker>, StoreError> {
        let limit = query.limit.unwrap_or(250).clamp(1, 10_000);
        let rows = sqlx::query(
            r#"
            SELECT occurred_at_ms, marker_type, label, details_json
            FROM app_events
            WHERE (?1 IS NULL OR occurred_at_ms >= ?1)
              AND (?2 IS NULL OR occurred_at_ms <= ?2)
            ORDER BY occurred_at_ms DESC
            LIMIT ?3
            "#,
        )
        .bind(query.since_ms)
        .bind(query.until_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut markers = rows
            .into_iter()
            .map(row_to_marker)
            .collect::<Result<Vec<_>, _>>()?;

        markers.extend(
            self.read_coverage_gap_markers(query, expected_gap_ms)
                .await?,
        );
        markers.sort_by_key(|marker| marker.occurred_at_ms);

        if markers.len() > limit as usize {
            let remove_count = markers.len() - limit as usize;
            markers.drain(0..remove_count);
        }

        Ok(markers)
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
            SELECT
              COUNT(*) AS rollup_bucket_count,
              MIN(first_captured_at_ms) AS rollup_oldest_captured_at_ms,
              MAX(newest_captured_at_ms) AS rollup_newest_captured_at_ms
            FROM metric_rollups_1m
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        let snapshot_json_row = sqlx::query(
            "SELECT COUNT(*) AS sample_count, MIN(captured_at_ms) AS oldest_ms FROM metric_samples WHERE snapshot_json IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await?;
        let snapshot_json_sample_count = snapshot_json_row.try_get("sample_count")?;
        let snapshot_json_oldest_ms = snapshot_json_row.try_get("oldest_ms")?;
        let l3_enabled = self
            .history_state_get::<bool>("l3Enabled")
            .await?
            .unwrap_or(settings.retention_ladder.l3.enabled);
        let l4_enabled = self
            .history_state_get::<bool>("l4Enabled")
            .await?
            .unwrap_or(settings.retention_ladder.l4.enabled);
        let mut tiers = Vec::with_capacity(4);
        for (tier, keep_days, enabled) in [
            (Tier::L1, settings.retention_ladder.l1.keep_days, true),
            (Tier::L2, settings.retention_ladder.l2.keep_days, true),
            (Tier::L3, settings.retention_ladder.l3.keep_days, l3_enabled),
            (Tier::L4, settings.retention_ladder.l4.keep_days, l4_enabled),
        ] {
            let time_column = if tier == Tier::L1 {
                "captured_at_ms"
            } else {
                "bucket_start_ms"
            };
            let sql = format!(
                "SELECT COUNT(*) AS bucket_count, MIN({time_column}) AS oldest_ms, MAX({time_column}) AS newest_ms FROM {}",
                tier.table()
            );
            let tier_row = sqlx::query(AssertSqlSafe(sql))
                .fetch_one(&self.pool)
                .await?;
            tiers.push(HistoryTierCoverage {
                tier: match tier {
                    Tier::L1 => "l1",
                    Tier::L2 => "l2",
                    Tier::L3 => "l3",
                    Tier::L4 => "l4",
                }
                .to_string(),
                enabled,
                keep_days,
                resolution_ms: if tier == Tier::L1 {
                    settings.poll_interval_ms
                } else {
                    tier.resolution_ms()
                },
                bucket_count: tier_row.try_get("bucket_count")?,
                oldest_ms: tier_row.try_get("oldest_ms")?,
                newest_ms: tier_row.try_get("newest_ms")?,
            });
        }
        let database_bytes = self.database_bytes().await?;
        let database_budget_percent = if settings.target_database_bytes > 0 {
            (database_bytes as f64 / settings.target_database_bytes as f64) * 100.0
        } else {
            0.0
        };
        let disk_pressure = self
            .history_state_get::<DiskPressureState>("diskPressure")
            .await?;
        let last_check_ms = self.history_state_get::<i64>("lastDiskCheckMs").await?;
        let migration = self
            .history_state_get::<JsonValue>("schemaMigration")
            .await?;
        let exported_until_month = self
            .history_state_get::<String>("coldExportedUntilMonth")
            .await?;
        let archive_paths =
            archive::archive_paths(self.database_path(), &settings.retention_ladder.archive);
        let (archive_bucket_count, archive_oldest_ms, archive_newest_ms) =
            archive::archive_coverage(&archive_paths).await?;
        let archive_manifest = archive::read_archive_manifest(&archive_paths).await?;
        let cold_file_count = i64::try_from(archive_manifest.len()).unwrap_or(i64::MAX);
        let cold_bytes = archive_manifest
            .iter()
            .fold(0_i64, |total, row| total.saturating_add(row.bytes));

        Ok(HistoryCoverage {
            sample_count: stats.sample_count,
            oldest_captured_at_ms: stats.oldest_captured_at_ms,
            newest_captured_at_ms: stats.newest_captured_at_ms,
            retention_hours: settings.retention_hours,
            rollup_retention_days: settings.rollup_retention_days,
            rollup_bucket_count: row.try_get::<i64, _>("rollup_bucket_count")?,
            database_bytes,
            target_database_bytes: settings.target_database_bytes,
            database_budget_percent,
            rollup_oldest_captured_at_ms: row
                .try_get::<Option<i64>, _>("rollup_oldest_captured_at_ms")?,
            rollup_newest_captured_at_ms: row
                .try_get::<Option<i64>, _>("rollup_newest_captured_at_ms")?,
            tiers,
            snapshot_json_oldest_ms,
            snapshot_json_sample_count,
            detail_interval_sec: settings.retention_ladder.detail_interval_sec,
            disk: HistoryDiskCoverage {
                free_bytes: disk_pressure.as_ref().map(|state| state.free_bytes),
                min_free_bytes: settings.retention_ladder.disk_check.min_free_bytes,
                pressure: disk_pressure.as_ref().is_some_and(|state| state.active),
                pressure_since_ms: disk_pressure
                    .as_ref()
                    .filter(|state| state.active)
                    .and_then(|state| state.since_ms),
                last_check_ms,
            },
            archive: HistoryArchiveCoverage {
                queryable: HistoryQueryableArchiveCoverage {
                    enabled: settings.retention_ladder.archive.queryable,
                    path: archive_paths.db.display().to_string(),
                    bucket_count: archive_bucket_count,
                    oldest_ms: archive_oldest_ms,
                    newest_ms: archive_newest_ms,
                },
                cold: HistoryColdArchiveCoverage {
                    enabled: settings.retention_ladder.archive.cold,
                    directory: archive_paths.directory.display().to_string(),
                    exported_until_month,
                    file_count: cold_file_count,
                    bytes: cold_bytes,
                },
            },
            otel: None,
            migration,
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

        Ok(result.rows_affected())
    }

    pub async fn count_rows_older_than(
        &self,
        tier: Tier,
        cutoff_ms: i64,
    ) -> Result<i64, StoreError> {
        if tier == Tier::L1 {
            return Ok(sqlx::query_scalar(
                "SELECT COUNT(*) FROM metric_samples WHERE captured_at_ms < ?",
            )
            .bind(cutoff_ms)
            .fetch_one(&self.pool)
            .await?);
        }

        let sql = format!(
            "SELECT COUNT(*) FROM {} WHERE bucket_start_ms + ? <= ?",
            tier.table()
        );
        Ok(sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(tier.resolution_ms())
            .bind(cutoff_ms)
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn count_snapshot_json_older_than(&self, cutoff_ms: i64) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM metric_samples WHERE snapshot_json IS NOT NULL AND captured_at_ms < ?",
        )
        .bind(cutoff_ms)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn prune_rollups(&self, tier: Tier, cutoff_end_ms: i64) -> Result<u64, StoreError> {
        if tier == Tier::L1 {
            return Err(StoreError::Validation(
                "Tier::L1 is pruned through prune_raw_history, not prune_rollups".to_string(),
            ));
        }
        let sql = format!(
            "DELETE FROM {} WHERE bucket_start_ms + ? <= ?",
            tier.table()
        );
        let result = sqlx::query(AssertSqlSafe(sql))
            .bind(tier.resolution_ms())
            .bind(cutoff_end_ms)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub(crate) async fn strip_snapshot_json(
        &self,
        cutoff_ms: i64,
        limit: i64,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE metric_samples
            SET snapshot_json = NULL
            WHERE rowid IN (
              SELECT rowid FROM metric_samples
              WHERE captured_at_ms < ? AND snapshot_json IS NOT NULL
              ORDER BY captured_at_ms
              LIMIT ?
            )
            "#,
        )
        .bind(cutoff_ms)
        .bind(limit)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub(crate) async fn prune_detail_history(&self, cutoff_ms: i64) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let fs = sqlx::query("DELETE FROM fs_samples WHERE captured_at_ms < ?")
            .bind(cutoff_ms)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        let processes = sqlx::query("DELETE FROM process_samples WHERE captured_at_ms < ?")
            .bind(cutoff_ms)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        transaction.commit().await?;
        Ok(fs.saturating_add(processes))
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

    async fn read_tier_history_points(
        &self,
        tier: Tier,
        query: HistoryPointsQuery,
    ) -> Result<Vec<HistoryPoint>, StoreError> {
        let limit = effective_points_limit(&query);
        if tier == Tier::L1 {
            let rows = sqlx::query(
                r#"
            SELECT
              captured_at_ms,
              cpu_usage_percent,
              memory_used_percent,
              swap_used_percent,
              load_percent,
              root_used_percent
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

            let mut points = rows
                .into_iter()
                .map(|row| {
                    Ok(HistoryPoint {
                        captured_at_ms: row.try_get::<i64, _>("captured_at_ms")?,
                        source: HistoryPointSource::Raw,
                        sample_count: 1,
                        cpu_usage_percent: row.try_get::<f64, _>("cpu_usage_percent")?,
                        memory_used_percent: row.try_get::<f64, _>("memory_used_percent")?,
                        swap_used_percent: row.try_get::<f64, _>("swap_used_percent")?,
                        load_percent: row.try_get::<f64, _>("load_percent")?,
                        root_used_percent: row.try_get::<Option<f64>, _>("root_used_percent")?,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            points.reverse();
            return Ok(points);
        }

        let sql = format!(
            r#"
            SELECT
              newest_captured_at_ms,
              sample_count,
              avg_cpu_usage_percent,
              avg_memory_used_percent,
              avg_swap_used_percent,
              avg_load_percent,
              avg_root_used_percent
            FROM {}
            WHERE (?1 IS NULL OR newest_captured_at_ms >= ?1)
              AND (?2 IS NULL OR newest_captured_at_ms <= ?2)
            ORDER BY newest_captured_at_ms DESC
            LIMIT ?3
            "#,
            tier.table()
        );
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(query.since_ms)
            .bind(query.until_ms)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        let mut points = rows
            .into_iter()
            .map(|row| {
                Ok(HistoryPoint {
                    captured_at_ms: row.try_get::<i64, _>("newest_captured_at_ms")?,
                    source: match tier {
                        Tier::L1 => unreachable!("raw points return above"),
                        Tier::L2 => HistoryPointSource::Rollup,
                        Tier::L3 => HistoryPointSource::Rollup5m,
                        Tier::L4 => HistoryPointSource::Rollup1h,
                    },
                    sample_count: row.try_get::<i64, _>("sample_count")?,
                    cpu_usage_percent: row.try_get::<f64, _>("avg_cpu_usage_percent")?,
                    memory_used_percent: row.try_get::<f64, _>("avg_memory_used_percent")?,
                    swap_used_percent: row.try_get::<f64, _>("avg_swap_used_percent")?,
                    load_percent: row.try_get::<f64, _>("avg_load_percent")?,
                    root_used_percent: row.try_get::<Option<f64>, _>("avg_root_used_percent")?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        points.reverse();
        Ok(points)
    }

    async fn read_coverage_gap_markers(
        &self,
        query: HistoryQuery,
        expected_gap_ms: i64,
    ) -> Result<Vec<HistoryMarker>, StoreError> {
        if expected_gap_ms <= 0 {
            return Ok(Vec::new());
        }

        let limit = query.limit.unwrap_or(250).clamp(2, 10_000);
        let rows = sqlx::query(
            r#"
            SELECT captured_at_ms
            FROM metric_samples
            WHERE (?1 IS NULL OR captured_at_ms >= ?1)
              AND (?2 IS NULL OR captured_at_ms <= ?2)
            ORDER BY captured_at_ms ASC
            LIMIT ?3
            "#,
        )
        .bind(query.since_ms)
        .bind(query.until_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut markers = Vec::new();
        let mut previous = None;
        for row in rows {
            let captured_at_ms = row.try_get::<i64, _>("captured_at_ms")?;
            if let Some(previous_captured_at_ms) = previous {
                let gap_ms = captured_at_ms.saturating_sub(previous_captured_at_ms);
                if gap_ms > expected_gap_ms {
                    markers.push(HistoryMarker {
                        occurred_at_ms: captured_at_ms,
                        marker_type: HistoryMarkerType::CoverageGap,
                        label: format!("Coverage gap {}", format_duration_short(gap_ms)),
                        details: serde_json::json!({
                            "fromMs": previous_captured_at_ms,
                            "toMs": captured_at_ms,
                            "gapMs": gap_ms,
                        }),
                    });
                }
            }
            previous = Some(captured_at_ms);
        }

        Ok(markers)
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
              MIN(cpu_usage_percent) AS min_cpu_usage_percent,
              MAX(cpu_usage_percent) AS max_cpu_usage_percent,
              AVG(memory_used_percent) AS avg_memory_used_percent,
              MIN(memory_used_percent) AS min_memory_used_percent,
              MAX(memory_used_percent) AS max_memory_used_percent,
              AVG(swap_used_percent) AS avg_swap_used_percent,
              MIN(swap_used_percent) AS min_swap_used_percent,
              MAX(swap_used_percent) AS max_swap_used_percent,
              AVG(load_percent) AS avg_load_percent,
              MIN(load_percent) AS min_load_percent,
              MAX(load_percent) AS max_load_percent,
              AVG(root_used_percent) AS avg_root_used_percent,
              MIN(root_used_percent) AS min_root_used_percent,
              MAX(root_used_percent) AS max_root_used_percent
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
              min_cpu_usage_percent,
              max_cpu_usage_percent,
              avg_memory_used_percent,
              min_memory_used_percent,
              max_memory_used_percent,
              avg_swap_used_percent,
              min_swap_used_percent,
              max_swap_used_percent,
              avg_load_percent,
              min_load_percent,
              max_load_percent,
              avg_root_used_percent,
              min_root_used_percent,
              max_root_used_percent
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_start_ms) DO UPDATE SET
              first_captured_at_ms = excluded.first_captured_at_ms,
              newest_captured_at_ms = excluded.newest_captured_at_ms,
              sample_count = excluded.sample_count,
              avg_cpu_usage_percent = excluded.avg_cpu_usage_percent,
              min_cpu_usage_percent = excluded.min_cpu_usage_percent,
              max_cpu_usage_percent = excluded.max_cpu_usage_percent,
              avg_memory_used_percent = excluded.avg_memory_used_percent,
              min_memory_used_percent = excluded.min_memory_used_percent,
              max_memory_used_percent = excluded.max_memory_used_percent,
              avg_swap_used_percent = excluded.avg_swap_used_percent,
              min_swap_used_percent = excluded.min_swap_used_percent,
              max_swap_used_percent = excluded.max_swap_used_percent,
              avg_load_percent = excluded.avg_load_percent,
              min_load_percent = excluded.min_load_percent,
              max_load_percent = excluded.max_load_percent,
              avg_root_used_percent = excluded.avg_root_used_percent,
              min_root_used_percent = excluded.min_root_used_percent,
              max_root_used_percent = excluded.max_root_used_percent
            "#,
        )
        .bind(bucket_start_ms)
        .bind(row.try_get::<i64, _>("first_captured_at_ms")?)
        .bind(row.try_get::<i64, _>("newest_captured_at_ms")?)
        .bind(sample_count)
        .bind(row.try_get::<f64, _>("avg_cpu_usage_percent")?)
        .bind(row.try_get::<f64, _>("min_cpu_usage_percent")?)
        .bind(row.try_get::<f64, _>("max_cpu_usage_percent")?)
        .bind(row.try_get::<f64, _>("avg_memory_used_percent")?)
        .bind(row.try_get::<f64, _>("min_memory_used_percent")?)
        .bind(row.try_get::<f64, _>("max_memory_used_percent")?)
        .bind(row.try_get::<f64, _>("avg_swap_used_percent")?)
        .bind(row.try_get::<f64, _>("min_swap_used_percent")?)
        .bind(row.try_get::<f64, _>("max_swap_used_percent")?)
        .bind(row.try_get::<f64, _>("avg_load_percent")?)
        .bind(row.try_get::<f64, _>("min_load_percent")?)
        .bind(row.try_get::<f64, _>("max_load_percent")?)
        .bind(row.try_get::<Option<f64>, _>("avg_root_used_percent")?)
        .bind(row.try_get::<Option<f64>, _>("min_root_used_percent")?)
        .bind(row.try_get::<Option<f64>, _>("max_root_used_percent")?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Canonicalize any legacy `runtime_kind` values written before the store
    /// switched from `format!("{:?}", kind)` to `RuntimeKind::as_str()` (M4).
    ///
    /// Older rows persisted the `Debug` names (`"Wsl"`, `"MacOs"`) which diverge
    /// from the serde/JSON contract (`"WSL"`, `"macOS"`). The remaining variants
    /// (`Linux`, `Windows`, `Unknown`) are identical in both forms, so only the
    /// two divergent values need rewriting. Both statements are idempotent: after
    /// the first run there are no matching rows, so re-running on every connect is
    /// a no-op.
    async fn migrate_runtime_kind_to_canonical(&self) -> Result<(), StoreError> {
        sqlx::query("UPDATE metric_samples SET runtime_kind = ? WHERE runtime_kind = ?")
            .bind("WSL")
            .bind("Wsl")
            .execute(&self.pool)
            .await?;
        sqlx::query("UPDATE metric_samples SET runtime_kind = ? WHERE runtime_kind = ?")
            .bind("macOS")
            .bind("MacOs")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn apply_pragmas(&self) -> Result<(), StoreError> {
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

        Ok(())
    }
}

pub fn database_path_from_url(database_url: &str) -> Result<PathBuf, StoreError> {
    let options = SqliteConnectOptions::from_str(database_url)?;
    Ok(options.get_filename().to_path_buf())
}

pub fn inspect_database_path(database_url: &str) -> Result<Option<PathBuf>, StoreError> {
    let database_path = database_path_from_url(database_url)?;
    match std::fs::metadata(&database_path) {
        Ok(_) => migration::canonical_database_path(&database_path).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StoreError::Migration {
            reason: format!(
                "cannot inspect database path {}: {error}",
                database_path.display()
            ),
            remedy: "make the database path accessible and retry; no database was created"
                .to_string(),
        }),
    }
}

pub fn effective_points_limit(query: &HistoryPointsQuery) -> i64 {
    query
        .limit
        .map(|limit| limit.clamp(1, 10_000))
        .unwrap_or(10_000)
}

/// Resolve `auto` without consulting SQLite so selection remains deterministic
/// and table-testable. Explicit sources pass through unchanged.
pub fn resolve_history_point_source(
    ladder: &RetentionLadder,
    now_ms: i64,
    query: HistoryPointsQuery,
) -> HistoryPointMode {
    resolve_history_point_source_with_poll(
        ladder,
        DashboardSettings::default().poll_interval_ms,
        now_ms,
        query,
    )
}

pub fn resolve_history_point_source_with_poll(
    ladder: &RetentionLadder,
    poll_interval_ms: i64,
    now_ms: i64,
    query: HistoryPointsQuery,
) -> HistoryPointMode {
    if query.source != HistoryPointMode::Auto {
        return query.source;
    }

    const DAY_MS: i64 = 86_400_000;
    let since_ms = query.since_ms.unwrap_or(now_ms);
    let until_ms = query.until_ms.unwrap_or(now_ms);
    let range_ms = until_ms.saturating_sub(since_ms).max(0);
    let limit = effective_points_limit(&query);
    let candidates = [
        (
            HistoryPointMode::Raw,
            ladder.l1.keep_days,
            true,
            poll_interval_ms.max(1),
        ),
        (
            HistoryPointMode::Rollup,
            ladder.l2.keep_days,
            true,
            Tier::L2.resolution_ms(),
        ),
        (
            HistoryPointMode::Rollup5m,
            ladder.l3.keep_days,
            ladder.l3.enabled,
            Tier::L3.resolution_ms(),
        ),
        (
            HistoryPointMode::Rollup1h,
            ladder.l4.keep_days,
            ladder.l4.enabled,
            Tier::L4.resolution_ms(),
        ),
    ];

    let mut coarsest_holding = None;
    for (source, keep_days, enabled, fixed_resolution_ms) in candidates {
        if !enabled {
            continue;
        }
        let holds_start = source == HistoryPointMode::Rollup1h && keep_days == 0
            || since_ms >= now_ms.saturating_sub(keep_days.saturating_mul(DAY_MS));
        if !holds_start {
            continue;
        }
        coarsest_holding = Some(source);
        if (range_ms / fixed_resolution_ms).saturating_add(1) <= limit {
            return source;
        }
    }

    if let Some(source) = coarsest_holding {
        return source;
    }
    if ladder.archive.queryable {
        return HistoryPointMode::Archive;
    }
    if ladder.l4.enabled {
        HistoryPointMode::Rollup1h
    } else if ladder.l3.enabled {
        HistoryPointMode::Rollup5m
    } else {
        HistoryPointMode::Rollup
    }
}

#[derive(Debug)]
pub enum StoreError {
    Sqlx(sqlx::Error),
    Json(serde_json::Error),
    IntegerOverflow {
        field: &'static str,
    },
    Migration {
        reason: String,
        remedy: String,
    },
    Archive {
        step: &'static str,
        source: ArchiveErrorSource,
    },
    DiskCheck {
        path: PathBuf,
        source: std::io::Error,
    },
    Validation(String),
}

#[derive(Debug)]
pub enum ArchiveErrorSource {
    Sqlx(sqlx::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for ArchiveErrorSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ArchiveErrorSource {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::IntegerOverflow { field } => {
                write!(formatter, "{field} does not fit in SQLite INTEGER")
            }
            Self::Migration { reason, remedy } => {
                write!(formatter, "migration refused: {reason}; remedy: {remedy}")
            }
            Self::Archive { step, source } => write!(
                formatter,
                "archive step {step} failed: {source}; remedy: {}",
                archive::archive_remedy(step)
            ),
            Self::DiskCheck { path, source } => write!(
                formatter,
                "disk check: cannot determine free bytes at {}: {source}; the last known disk-pressure state is kept — check the mount or set retentionLadder.diskCheck.minFreeBytes",
                path.display()
            ),
            Self::Validation(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Archive { source, .. } => match source {
                ArchiveErrorSource::Sqlx(error) => Some(error),
                ArchiveErrorSource::Io(error) => Some(error),
            },
            Self::DiskCheck { source, .. } => Some(source),
            Self::IntegerOverflow { .. } | Self::Migration { .. } | Self::Validation(_) => None,
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
    let snapshot_json = row
        .try_get::<Option<String>, _>("snapshot_json")?
        .ok_or_else(|| {
            StoreError::Validation(format!(
                "metric_samples row at {captured_at_ms} has no snapshot_json"
            ))
        })?;
    let snapshot = serde_json::from_str(&snapshot_json)?;
    Ok(HistorySample {
        captured_at_ms,
        snapshot,
    })
}

fn tier_bucket_from_row(row: sqlx::sqlite::SqliteRow) -> Result<TierBucket, StoreError> {
    let stat =
        |avg: &'static str, min: &'static str, max: &'static str| -> Result<Stat, StoreError> {
            Ok(Stat {
                avg: row.try_get(avg)?,
                min: row.try_get(min)?,
                max: row.try_get(max)?,
            })
        };
    let root_used = row
        .try_get::<Option<f64>, _>("avg_root_used_percent")?
        .map(|avg| {
            Ok::<Stat, StoreError>(Stat {
                avg,
                min: row
                    .try_get::<Option<f64>, _>("min_root_used_percent")?
                    .unwrap_or(avg),
                max: row
                    .try_get::<Option<f64>, _>("max_root_used_percent")?
                    .unwrap_or(avg),
            })
        })
        .transpose()?;
    Ok(TierBucket {
        bucket_start_ms: row.try_get("bucket_start_ms")?,
        first_captured_at_ms: row.try_get("first_captured_at_ms")?,
        newest_captured_at_ms: row.try_get("newest_captured_at_ms")?,
        sample_count: row.try_get("sample_count")?,
        cpu: stat(
            "avg_cpu_usage_percent",
            "min_cpu_usage_percent",
            "max_cpu_usage_percent",
        )?,
        memory: stat(
            "avg_memory_used_percent",
            "min_memory_used_percent",
            "max_memory_used_percent",
        )?,
        swap: stat(
            "avg_swap_used_percent",
            "min_swap_used_percent",
            "max_swap_used_percent",
        )?,
        load: stat("avg_load_percent", "min_load_percent", "max_load_percent")?,
        root_used,
    })
}

fn history_point_from_bucket(bucket: TierBucket, source: HistoryPointSource) -> HistoryPoint {
    HistoryPoint {
        captured_at_ms: bucket.newest_captured_at_ms,
        source,
        sample_count: bucket.sample_count,
        cpu_usage_percent: bucket.cpu.avg,
        memory_used_percent: bucket.memory.avg,
        swap_used_percent: bucket.swap.avg,
        load_percent: bucket.load.avg,
        root_used_percent: bucket.root_used.map(|stat| stat.avg),
    }
}

fn row_to_marker(row: sqlx::sqlite::SqliteRow) -> Result<HistoryMarker, StoreError> {
    let details_json = row.try_get::<String, _>("details_json")?;
    Ok(HistoryMarker {
        occurred_at_ms: row.try_get::<i64, _>("occurred_at_ms")?,
        marker_type: HistoryMarkerType::from_storage(&row.try_get::<String, _>("marker_type")?)?,
        label: row.try_get::<String, _>("label")?,
        details: serde_json::from_str(&details_json)?,
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

pub(crate) fn validate_one_of(
    field: &str,
    value: &str,
    allowed: &[&str],
) -> Result<(), StoreError> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(StoreError::Validation(format!(
        "{field} must be one of {}",
        allowed.join(", ")
    )))
}

pub(crate) fn validate_range(
    field: &str,
    value: i64,
    min: i64,
    max: i64,
) -> Result<(), StoreError> {
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

/// TinyTop's canonical normalized one-minute load percentage.
///
/// Exporters reuse this helper so stored history and outbound telemetry cannot
/// drift on zero-core handling or the 0–100 clamp.
pub fn load_percent(snapshot: &SystemSnapshot) -> f64 {
    if snapshot.cpu.cores == 0 {
        0.0
    } else {
        ((snapshot.load.one / snapshot.cpu.cores as f64) * 100.0).clamp(0.0, 100.0)
    }
}

fn bucket_start_ms(captured_at_ms: i64) -> i64 {
    captured_at_ms.div_euclid(60_000).saturating_mul(60_000)
}

fn format_duration_short(duration_ms: i64) -> String {
    let total_seconds = duration_ms.saturating_div(1_000).max(0);
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }
    let total_minutes = total_seconds / 60;
    if total_minutes < 60 {
        return format!("{total_minutes}m");
    }
    let total_hours = total_minutes / 60;
    if total_hours < 24 {
        return format!("{total_hours}h");
    }
    format!("{}d", total_hours / 24)
}

fn default_target_database_bytes() -> i64 {
    128 * 1024 * 1024
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
