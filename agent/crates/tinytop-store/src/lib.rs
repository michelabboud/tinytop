use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use tinytop_types::SystemSnapshot;

#[derive(Debug, Clone, PartialEq)]
pub struct HistorySample {
    pub captured_at_ms: i64,
    pub snapshot: SystemSnapshot,
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
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await?;
        let store = Self { pool };
        store.apply_schema().await?;
        Ok(store)
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

        Ok(())
    }
}

#[derive(Debug)]
pub enum StoreError {
    Sqlx(sqlx::Error),
    Json(serde_json::Error),
    IntegerOverflow { field: &'static str },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::IntegerOverflow { field } => {
                write!(formatter, "{field} does not fit in SQLite INTEGER")
            }
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::IntegerOverflow { .. } => None,
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

fn load_percent(snapshot: &SystemSnapshot) -> f64 {
    if snapshot.cpu.cores == 0 {
        0.0
    } else {
        ((snapshot.load.one / snapshot.cpu.cores as f64) * 100.0).clamp(0.0, 100.0)
    }
}
