use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use sqlx::{
    Connection, Row, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tinytop_store::{
    ArchiveErrorSource, DashboardSettings, HistoryPointMode, HistoryPointSource,
    HistoryPointsQuery, SqliteHistoryStore, StoreError,
    archive::{
        ArchiveSchemaState, archive_paths, archive_schema_state, copy_expired_l4_batch,
        export_cold_months, move_expired_l4, read_archive_manifest, read_archive_points,
        verify_cold_file,
    },
    ladder::{Stat, Tier, TierBucket},
    maintenance::maintain,
};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;
const AUG_29_2026_MS: i64 = 1_787_961_600_000;
const SEP_2_2026_MS: i64 = 1_788_307_200_000;
const JAN_2023_MS: i64 = 1_672_531_200_000;
const FEB_2023_MS: i64 = 1_675_209_600_000;
const MAR_2023_MS: i64 = 1_677_628_800_000;
const JUL_2026_MS: i64 = 1_782_864_000_000;

struct TempDatabase {
    dir: PathBuf,
    url: String,
    preserve: bool,
}

impl TempDatabase {
    fn new(prefix: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-archive-{prefix}-{}-{stamp}",
            std::process::id()
        ));
        assert!(dir.starts_with(std::env::temp_dir()));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("history.sqlite");
        let url = format!("sqlite://{}", path.display());
        Self {
            dir,
            url,
            preserve: false,
        }
    }

    async fn store(&self) -> SqliteHistoryStore {
        SqliteHistoryStore::connect(&self.url)
            .await
            .expect("fixture store should connect")
    }

    async fn pool(&self) -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(2)
            .connect(&self.url)
            .await
            .expect("fixture verification pool should connect")
    }

    fn preserve(&mut self) {
        self.preserve = true;
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        if !self.preserve {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }
}

fn bucket(start_ms: i64, value: f64) -> TierBucket {
    let stat = Stat {
        avg: value,
        min: value,
        max: value,
    };
    TierBucket {
        bucket_start_ms: start_ms,
        first_captured_at_ms: start_ms,
        newest_captured_at_ms: start_ms + HOUR_MS - 1,
        sample_count: 60,
        cpu: stat,
        memory: stat,
        swap: stat,
        load: stat,
        root_used: Some(stat),
    }
}

async fn seed_l4(store: &SqliteHistoryStore, starts: &[i64]) {
    for (index, start_ms) in starts.iter().copied().enumerate() {
        store
            .upsert_tier_bucket(Tier::L4, &bucket(start_ms, 10.0 + index as f64))
            .await
            .expect("L4 fixture bucket should insert");
    }
}

fn cold_ladder() -> tinytop_store::retention_ladder::RetentionLadder {
    let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
    ladder.l3.enabled = false;
    ladder.l4.keep_days = 30;
    ladder.archive.queryable = true;
    ladder.archive.cold = true;
    ladder.archive.cold_after_months = 1;
    ladder
}

async fn seed_archive(
    store: &SqliteHistoryStore,
    paths: &tinytop_store::archive::ArchivePaths,
    starts: &[i64],
) {
    seed_l4(store, starts).await;
    assert_eq!(
        move_expired_l4(store, paths, i64::MAX, starts.len().max(1))
            .await
            .expect("fixture L4 rows should move to the archive"),
        starts.len() as i64
    );
}

async fn archive_l4_count(paths: &tinytop_store::archive::ArchivePaths) -> i64 {
    let pool = read_only_archive_pool(&paths.db).await;
    let count = sqlx::query_scalar("SELECT COUNT(*) FROM metric_rollups_1h")
        .fetch_one(&pool)
        .await
        .expect("archive row count should query");
    pool.close().await;
    count
}

fn read_csv_gz(path: &std::path::Path) -> String {
    let file = std::fs::File::open(path).expect("cold file should open");
    let mut decoder = GzDecoder::new(file);
    let mut csv = String::new();
    decoder
        .read_to_string(&mut csv)
        .expect("cold file should decompress as UTF-8 CSV");
    csv
}

fn write_csv_gz(path: &std::path::Path, csv: &str) {
    let file = std::fs::File::create(path).expect("cold fixture should create");
    let mut encoder = GzEncoder::new(file, Compression::new(6));
    encoder
        .write_all(csv.as_bytes())
        .expect("cold fixture CSV should compress");
    encoder
        .finish()
        .expect("cold fixture gzip should finish")
        .sync_all()
        .expect("cold fixture gzip should sync");
}

fn verifier_fixture_csv(first_field: &str) -> String {
    let header = "bucket_start_ms,first_captured_at_ms,newest_captured_at_ms,sample_count,avg_cpu_usage_percent,min_cpu_usage_percent,max_cpu_usage_percent,avg_memory_used_percent,min_memory_used_percent,max_memory_used_percent,avg_swap_used_percent,min_swap_used_percent,max_swap_used_percent,avg_load_percent,min_load_percent,max_load_percent,avg_root_used_percent,min_root_used_percent,max_root_used_percent";
    format!("{header}\r\n{first_field},{}\r\n", ["1"; 18].join(","))
}

fn parse_rfc4180(csv: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = csv.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {
                assert_eq!(chars.next(), Some('\n'), "records must end with CRLF");
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\n' => panic!("records must not use bare LF"),
            _ => field.push(ch),
        }
    }
    assert!(!quoted, "quoted field must close");
    assert!(
        field.is_empty() && row.is_empty(),
        "last record must end in CRLF"
    );
    rows
}

fn bucket_fields(bucket: &TierBucket) -> Vec<String> {
    let mut fields = vec![
        bucket.bucket_start_ms.to_string(),
        bucket.first_captured_at_ms.to_string(),
        bucket.newest_captured_at_ms.to_string(),
        bucket.sample_count.to_string(),
        bucket.cpu.avg.to_string(),
        bucket.cpu.min.to_string(),
        bucket.cpu.max.to_string(),
        bucket.memory.avg.to_string(),
        bucket.memory.min.to_string(),
        bucket.memory.max.to_string(),
        bucket.swap.avg.to_string(),
        bucket.swap.min.to_string(),
        bucket.swap.max.to_string(),
        bucket.load.avg.to_string(),
        bucket.load.min.to_string(),
        bucket.load.max.to_string(),
    ];
    match bucket.root_used {
        Some(root) => {
            fields.push(root.avg.to_string());
            fields.push(root.min.to_string());
            fields.push(root.max.to_string());
        }
        None => fields.extend([String::new(), String::new(), String::new()]),
    }
    fields
}

async fn main_l4_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM metric_rollups_1h")
        .fetch_one(pool)
        .await
        .expect("fixture count should query")
}

async fn assert_only_main(connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>) {
    let rows = sqlx::query("PRAGMA database_list")
        .fetch_all(&mut **connection)
        .await
        .expect("database list should query");
    let names = rows
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<Vec<_>>();
    assert_eq!(names, ["main"]);
}

async fn create_sqlite_fixture(path: &std::path::Path, sql: &'static str) {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("SQLite fixture should open");
    sqlx::raw_sql(sql)
        .execute(&mut connection)
        .await
        .expect("SQLite fixture SQL should execute");
    connection
        .close()
        .await
        .expect("SQLite fixture should close");
}

async fn read_only_archive_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .expect("archive fixture should open read-only")
}

#[test]
fn archive_error_remedy_is_step_aware() {
    // Break caught: remedy text describes the wrong durable state for an archive phase.
    let watermark = StoreError::Archive {
        step: "watermark",
        source: ArchiveErrorSource::Io(std::io::Error::other("fixture watermark failure")),
    }
    .to_string();
    assert!(watermark.contains(
        "remedy: the batch's copy is committed in history-archive.sqlite and is refreshed on retry; nothing was deleted from the main database — retrying is safe"
    ));

    let insert = StoreError::Archive {
        step: "insert",
        source: ArchiveErrorSource::Io(std::io::Error::other("fixture insert failure")),
    }
    .to_string();
    assert!(insert.contains(
        "remedy: nothing was written to history-archive.sqlite and nothing was deleted from the main database; check the archive directory is writable and retry"
    ));

    let detach = StoreError::Archive {
        step: "detach",
        source: ArchiveErrorSource::Io(std::io::Error::other("fixture detach failure")),
    }
    .to_string();
    assert!(detach.contains(
        "remedy: the batch is committed in history-archive.sqlite and removed from the main database; only the detach bookkeeping failed — retrying is safe, nothing is duplicated or lost"
    ));

    let cold_verify = StoreError::Archive {
        step: "cold verify",
        source: ArchiveErrorSource::Io(std::io::Error::other("fixture cold verify failure")),
    }
    .to_string();
    assert!(cold_verify.contains(
        "remedy: the queryable archive is untouched; a `.tmp` file may remain in the archive directory and is safe to delete; retrying re-exports the month"
    ));
}

#[tokio::test]
async fn expired_l4_rows_move_and_main_rows_vanish_only_after_verified_insert() {
    // Break caught: a failed archive write deletes main L4 rows or advances the move watermark.
    let fixture = TempDatabase::new("verified-move");
    let store = fixture.store().await;
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.archive.queryable = true;
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_l4(&store, &[0, HOUR_MS, 2 * HOUR_MS, 4 * HOUR_MS]).await;

    assert_eq!(
        move_expired_l4(&store, &paths, 4 * HOUR_MS, 2)
            .await
            .expect("first archive batch should move"),
        2
    );
    let main_pool = fixture.pool().await;
    assert_eq!(main_l4_count(&main_pool).await, 2);
    assert_eq!(
        read_archive_points(&paths, i64::MIN, i64::MAX, 100)
            .await
            .expect("archive should read")
            .len(),
        2
    );
    assert_eq!(
        store
            .history_state_get::<i64>("archiveMovedUntilMs")
            .await
            .expect("watermark should read"),
        Some(2 * HOUR_MS)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&paths.db, std::fs::Permissions::from_mode(0o444))
            .expect("archive should become read-only");
        let error = move_expired_l4(&store, &paths, 4 * HOUR_MS, 2)
            .await
            .expect_err("read-only archive must reject the second batch");
        eprintln!("chmod fault injection error: {error}");
        assert!(
            error
                .to_string()
                .contains("remedy: nothing was written to history-archive.sqlite")
        );
        assert!(std::error::Error::source(&error).is_some());
        match &error {
            StoreError::Archive { step, .. } => {
                assert!(
                    ["attach", "insert", "commit copy"].contains(step),
                    "unexpected step {step}"
                );
            }
            other => panic!("expected archive error, got {other:?}"),
        }
        assert_eq!(main_l4_count(&main_pool).await, 2);
        assert_eq!(
            read_archive_points(&paths, i64::MIN, i64::MAX, 100)
                .await
                .expect("read-only archive should remain readable")
                .len(),
            2
        );
        assert_eq!(
            store
                .history_state_get::<i64>("archiveMovedUntilMs")
                .await
                .expect("watermark should read"),
            Some(2 * HOUR_MS)
        );
        assert_eq!(
            store
                .attached_database_names()
                .await
                .expect("store pool database list should query after failure"),
            ["main"]
        );
        let mut connection = main_pool.acquire().await.expect("pool connection");
        assert_only_main(&mut connection).await;
        drop(connection);

        std::fs::set_permissions(&paths.db, std::fs::Permissions::from_mode(0o644))
            .expect("archive permissions should restore");
        assert_eq!(
            move_expired_l4(&store, &paths, 4 * HOUR_MS, 2)
                .await
                .expect("retry should converge"),
            1
        );
        assert_eq!(
            store
                .attached_database_names()
                .await
                .expect("store pool database list should query after convergence"),
            ["main"]
        );
    }

    let mut maintenance_settings = DashboardSettings::default();
    maintenance_settings.retention_ladder.archive.queryable = true;
    let remaining_before_maintenance = main_l4_count(&main_pool).await;
    let report = maintain(&store, &maintenance_settings, 731 * DAY_MS)
        .await
        .expect("queryable maintenance should archive the remaining L4 row");
    assert_eq!(report.archived_l4, remaining_before_maintenance);
    assert_eq!(report.expired_l4, remaining_before_maintenance);
    assert_eq!(report.pruned[3], remaining_before_maintenance);
    assert_eq!(main_l4_count(&main_pool).await, 0);
}

#[tokio::test]
async fn archive_copy_is_committed_before_main_delete() {
    // Break caught: phase 1 writes main, deletes before the archive commit, or cannot converge.
    let fixture = TempDatabase::new("copy-before-delete");
    let store = fixture.store().await;
    let settings = DashboardSettings::default();
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_l4(&store, &[0, HOUR_MS, 2 * HOUR_MS]).await;

    let main_pool = fixture.pool().await;
    let mut observer = main_pool.acquire().await.expect("observer connection");
    let data_version_before: i64 = sqlx::query_scalar("PRAGMA main.data_version")
        .fetch_one(&mut *observer)
        .await
        .expect("main data version before copy");

    let copied = copy_expired_l4_batch(&store, &paths, 4 * HOUR_MS, 3)
        .await
        .expect("phase 1 copy should commit")
        .expect("three expired rows should form a batch");
    assert_eq!(copied.min_ms, 0);
    assert_eq!(copied.max_ms, 2 * HOUR_MS);
    assert_eq!(copied.row_count, 3);

    let data_version_after: i64 = sqlx::query_scalar("PRAGMA main.data_version")
        .fetch_one(&mut *observer)
        .await
        .expect("main data version after copy");
    assert_eq!(data_version_after, data_version_before);
    assert_eq!(main_l4_count(&main_pool).await, 3);
    let archived = read_archive_points(&paths, i64::MIN, i64::MAX, 100)
        .await
        .expect("committed archive copy should read");
    assert_eq!(archived.len(), 3);
    assert_eq!(
        store
            .history_state_get::<i64>("archiveMovedUntilMs")
            .await
            .expect("watermark should read"),
        None
    );
    drop(observer);

    sqlx::query("UPDATE metric_rollups_1h SET sample_count = 61 WHERE bucket_start_ms = ?1")
        .bind(HOUR_MS)
        .execute(&main_pool)
        .await
        .expect("main row should mutate between phases");
    sqlx::query(
        "UPDATE metric_rollups_1h SET avg_cpu_usage_percent = 99.0 WHERE bucket_start_ms = ?1",
    )
    .bind(0_i64)
    .execute(&main_pool)
    .await
    .expect("main payload should mutate between phases");

    assert_eq!(
        move_expired_l4(&store, &paths, 4 * HOUR_MS, 3)
            .await
            .expect("full move should refresh and delete the batch"),
        3
    );
    assert_eq!(main_l4_count(&main_pool).await, 0);
    let archived = read_archive_points(&paths, i64::MIN, i64::MAX, 100)
        .await
        .expect("converged archive should read");
    assert_eq!(archived.len(), 3);
    assert_eq!(
        archived
            .iter()
            .find(|bucket| bucket.bucket_start_ms == HOUR_MS)
            .expect("mutated bucket should be archived")
            .sample_count,
        61
    );
    assert_eq!(
        archived
            .iter()
            .find(|bucket| bucket.bucket_start_ms == 0)
            .expect("payload-only mutated bucket should be archived")
            .cpu
            .avg,
        99.0
    );
    assert_eq!(
        store
            .history_state_get::<i64>("archiveMovedUntilMs")
            .await
            .expect("watermark should read"),
        Some(3 * HOUR_MS)
    );
}

#[tokio::test]
async fn partial_batch_does_not_livelock_the_next_move() {
    // Break caught: verify counts stale archive keys inside the selected key range forever.
    let fixture = TempDatabase::new("partial-batch-next-move");
    let store = fixture.store().await;
    let settings = DashboardSettings::default();
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_l4(&store, &[0, HOUR_MS, 2 * HOUR_MS]).await;

    let copied = copy_expired_l4_batch(&store, &paths, 4 * HOUR_MS, 2)
        .await
        .expect("partial phase 1 copy should commit")
        .expect("two expired rows should form a batch");
    assert_eq!(copied.row_count, 2);

    let main_pool = fixture.pool().await;
    sqlx::query("DELETE FROM metric_rollups_1h WHERE bucket_start_ms = ?1")
        .bind(HOUR_MS)
        .execute(&main_pool)
        .await
        .expect("fixture should model a partial phase B delete");

    assert_eq!(
        move_expired_l4(&store, &paths, 4 * HOUR_MS, 10)
            .await
            .expect("next move should ignore extra archive keys in the selected interval"),
        2
    );
    assert_eq!(main_l4_count(&main_pool).await, 0);
    let archived_starts = read_archive_points(&paths, i64::MIN, i64::MAX, 100)
        .await
        .expect("converged archive should read")
        .into_iter()
        .map(|bucket| bucket.bucket_start_ms)
        .collect::<Vec<_>>();
    assert_eq!(archived_starts, [0, HOUR_MS, 2 * HOUR_MS]);
    assert_eq!(
        store
            .history_state_get::<i64>("archiveMovedUntilMs")
            .await
            .expect("watermark should read"),
        Some(3 * HOUR_MS)
    );
}

#[tokio::test]
async fn archive_directory_setting_relocates_the_file() {
    // Break caught: archive.directory is ignored and the archive lands beside the main DB.
    let fixture = TempDatabase::new("directory");
    let store = fixture.store().await;
    let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
    ladder.archive.queryable = true;
    let relocated = fixture.dir.join("relocated");
    ladder.archive.directory = relocated.display().to_string();
    let settings = DashboardSettings {
        retention_ladder: ladder,
        ..DashboardSettings::default()
    };
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_l4(&store, &[0]).await;

    assert_eq!(
        move_expired_l4(&store, &paths, 2 * HOUR_MS, 10)
            .await
            .expect("archive row should move"),
        1
    );
    assert_eq!(paths.directory, relocated);
    assert_eq!(paths.db, relocated.join("history-archive.sqlite"));
    assert!(paths.db.is_file());
    assert!(!fixture.dir.join("history-archive.sqlite").exists());
    let archive_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&paths.db)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .expect("archive schema should open read-only");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&archive_pool)
            .await
            .expect("archive version should query"),
        1
    );
    let object_names = sqlx::query_scalar::<_, String>(
        r#"
        SELECT name
        FROM sqlite_master
        WHERE name IN ('metric_rollups_1h', 'idx_metric_rollups_1h_newest', 'archive_manifest')
        ORDER BY name
        "#,
    )
    .fetch_all(&archive_pool)
    .await
    .expect("archive objects should query");
    assert_eq!(
        object_names,
        [
            "archive_manifest",
            "idx_metric_rollups_1h_newest",
            "metric_rollups_1h"
        ]
    );
}

#[tokio::test]
async fn archive_schema_refuses_foreign_or_newer_files() {
    // Break caught: schema setup stamps a future or unrelated SQLite file as archive v1.
    let newer = TempDatabase::new("schema-newer");
    let newer_store = newer.store().await;
    let settings = DashboardSettings::default();
    let newer_paths = archive_paths(
        newer_store.database_path(),
        &settings.retention_ladder.archive,
    );
    seed_l4(&newer_store, &[0]).await;
    create_sqlite_fixture(&newer_paths.db, "PRAGMA user_version = 2;").await;

    let newer_error = move_expired_l4(&newer_store, &newer_paths, 2 * HOUR_MS, 10)
        .await
        .expect_err("a newer archive schema must be refused");
    match newer_error {
        StoreError::Archive {
            step: "schema",
            source,
        } => {
            assert!(
                source
                    .to_string()
                    .contains("has user_version 2 with 0 objects")
            );
        }
        other => panic!("expected archive schema error, got {other:?}"),
    }
    let newer_main_pool = newer.pool().await;
    assert_eq!(main_l4_count(&newer_main_pool).await, 1);
    let newer_archive_pool = read_only_archive_pool(&newer_paths.db).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&newer_archive_pool)
            .await
            .expect("newer archive version should query"),
        2
    );
    newer_archive_pool.close().await;

    let foreign = TempDatabase::new("schema-foreign");
    let foreign_store = foreign.store().await;
    let foreign_paths = archive_paths(
        foreign_store.database_path(),
        &settings.retention_ladder.archive,
    );
    seed_l4(&foreign_store, &[0]).await;
    create_sqlite_fixture(&foreign_paths.db, "CREATE TABLE stranger (x);").await;

    let foreign_error = move_expired_l4(&foreign_store, &foreign_paths, 2 * HOUR_MS, 10)
        .await
        .expect_err("an unrelated user_version 0 database must be refused");
    match foreign_error {
        StoreError::Archive {
            step: "schema",
            source,
        } => {
            assert!(
                source
                    .to_string()
                    .contains("has user_version 0 with 1 objects")
            );
        }
        other => panic!("expected archive schema error, got {other:?}"),
    }
    let foreign_main_pool = foreign.pool().await;
    assert_eq!(main_l4_count(&foreign_main_pool).await, 1);
    let foreign_archive_pool = read_only_archive_pool(&foreign_paths.db).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&foreign_archive_pool)
            .await
            .expect("foreign archive version should query"),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'stranger'",
        )
        .fetch_one(&foreign_archive_pool)
        .await
        .expect("foreign table should query"),
        1
    );
}

#[tokio::test]
async fn auto_falls_through_to_archive_for_ranges_older_than_l4() {
    // Break caught: auto selects Archive but the store returns an empty stub or L4-labelled points.
    let fixture = TempDatabase::new("auto");
    let store = fixture.store().await;
    let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
    ladder.archive.queryable = true;
    let settings = DashboardSettings {
        retention_ladder: ladder,
        ..DashboardSettings::default()
    };
    store
        .put_settings(&settings)
        .await
        .expect("archive setting should persist");
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_l4(&store, &[0, HOUR_MS]).await;
    assert_eq!(
        move_expired_l4(&store, &paths, 3 * HOUR_MS, 10)
            .await
            .expect("archive rows should move"),
        2
    );

    let points = store
        .read_history_points(HistoryPointsQuery {
            since_ms: Some(0),
            until_ms: Some(3 * HOUR_MS),
            limit: Some(100),
            source: HistoryPointMode::Auto,
        })
        .await
        .expect("auto archive read should succeed");
    assert_eq!(points.len(), 2);
    assert!(
        points
            .iter()
            .all(|point| point.source == HistoryPointSource::Archive)
    );
    assert_eq!(
        HistoryPointMode::Archive.resolution_ms(settings.poll_interval_ms),
        HOUR_MS
    );
}

#[tokio::test]
async fn archive_is_never_attached_while_idle() {
    // Break caught: a successful move returns a pooled connection with archive still attached.
    let fixture = TempDatabase::new("detach");
    let store = fixture.store().await;
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.archive.queryable = true;
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_l4(&store, &[0]).await;

    let zero_copy = copy_expired_l4_batch(&store, &paths, 2 * HOUR_MS, 0)
        .await
        .expect("zero-sized copy batch should be a no-op");
    let zero_move = move_expired_l4(&store, &paths, 2 * HOUR_MS, 0)
        .await
        .expect("zero-sized move batch should be a no-op");
    assert_eq!((zero_copy, zero_move, paths.db.exists()), (None, 0, false));
    assert_eq!(
        move_expired_l4(&store, &paths, 2 * HOUR_MS, 10)
            .await
            .expect("archive row should move"),
        1
    );
    assert_eq!(
        store
            .attached_database_names()
            .await
            .expect("store pool database list should query after first move"),
        ["main"]
    );
    assert_eq!(
        move_expired_l4(&store, &paths, 2 * HOUR_MS, 10)
            .await
            .expect("same pooled connection should attach again cleanly"),
        0
    );
    assert_eq!(
        store
            .attached_database_names()
            .await
            .expect("store pool database list should query after second move"),
        ["main"]
    );

    let pool = fixture.pool().await;
    let mut first = pool.acquire().await.expect("first pool connection");
    let mut second = pool.acquire().await.expect("second pool connection");
    assert_only_main(&mut first).await;
    assert_only_main(&mut second).await;
}

#[tokio::test]
async fn expire_l4_deletes_when_archive_not_queryable() {
    // Break caught: ordinary L4 expiry creates or writes an archive despite queryable=false.
    let fixture = TempDatabase::new("delete");
    let store = fixture.store().await;
    let settings = DashboardSettings::default();
    let now_ms = 731 * DAY_MS;
    seed_l4(&store, &[0]).await;

    let report = maintain(&store, &settings, now_ms)
        .await
        .expect("delete-mode maintenance should succeed");
    assert_eq!(report.expired_l4, 1);
    assert_eq!(report.archived_l4, 0);
    assert_eq!(report.pruned[3], 1);
    assert!(!fixture.dir.join("history-archive.sqlite").exists());
}

#[tokio::test]
async fn coverage_reports_archive_counts_and_reads_never_create_the_file() {
    // Break caught: inspection creates an empty archive or coverage keeps reporting zero after moves.
    let fixture = TempDatabase::new("coverage");
    let store = fixture.store().await;
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.archive.queryable = true;
    store
        .put_settings(&settings)
        .await
        .expect("archive setting should persist");
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    assert!(!paths.db.exists());

    let empty_coverage = store
        .history_coverage(&settings)
        .await
        .expect("empty archive coverage should succeed");
    assert_eq!(empty_coverage.archive.queryable.bucket_count, 0);
    assert_eq!(empty_coverage.archive.queryable.oldest_ms, None);
    assert_eq!(empty_coverage.archive.queryable.newest_ms, None);
    assert_eq!(
        empty_coverage.archive.queryable.path,
        paths.db.display().to_string()
    );
    assert!(
        store
            .read_history_points(HistoryPointsQuery {
                since_ms: Some(0),
                until_ms: Some(2 * HOUR_MS),
                limit: Some(10),
                source: HistoryPointMode::Archive,
            })
            .await
            .expect("missing archive read should succeed")
            .is_empty()
    );
    assert!(!paths.db.exists());

    seed_l4(&store, &[0]).await;
    assert_eq!(
        move_expired_l4(&store, &paths, 2 * HOUR_MS, 10)
            .await
            .expect("archive row should move"),
        1
    );
    let populated_coverage = store
        .history_coverage(&settings)
        .await
        .expect("archive coverage should succeed");
    assert_eq!(populated_coverage.archive.queryable.bucket_count, 1);
    assert_eq!(populated_coverage.archive.queryable.oldest_ms, Some(0));
    assert_eq!(populated_coverage.archive.queryable.newest_ms, Some(0));

    settings.retention_ladder.archive.queryable = false;
    store
        .put_settings(&settings)
        .await
        .expect("disabled archive setting should persist");
    assert!(
        store
            .read_history_points(HistoryPointsQuery {
                since_ms: Some(0),
                until_ms: Some(2 * HOUR_MS),
                limit: Some(10),
                source: HistoryPointMode::Archive,
            })
            .await
            .expect("disabled archive read should succeed")
            .is_empty()
    );
}

// cold_requires_queryable: covered by tests/retention_settings.rs::retention_ladder_validation_rejects_every_invalid_shape_with_exact_error (cold=true, queryable=false).

#[tokio::test]
async fn cold_export_month_listing_agrees_with_month_bounds() {
    // Break caught: SQLite truncates negative millisecond timestamps into the wrong UTC month.
    let fixture = TempDatabase::new("cold-month-boundaries");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_archive(
        &store,
        &paths,
        &[-1, 0, 1_706_745_599_999, 1_706_745_600_000],
    )
    .await;

    let written = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("all four boundary months should export");
    assert_eq!(
        written
            .iter()
            .map(|row| (row.month.as_str(), row.row_count))
            .collect::<Vec<_>>(),
        [
            ("1969-12", 1),
            ("1970-01", 1),
            ("2024-01", 1),
            ("2024-02", 1),
        ]
    );
}

#[tokio::test]
async fn incomplete_archive_schema_is_reported_while_reads_stay_lenient() {
    // Break caught: status cannot distinguish an incomplete v1 archive from an empty current one.
    let fixture = TempDatabase::new("incomplete-schema-state");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_archive(&store, &paths, &[JAN_2023_MS]).await;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&paths.db)
            .create_if_missing(false),
    )
    .await
    .expect("archive fixture should open read-write");
    sqlx::query("DROP TABLE archive_manifest")
        .execute(&mut connection)
        .await
        .expect("fixture manifest should drop");
    connection
        .close()
        .await
        .expect("archive fixture should close");

    assert_eq!(
        archive_schema_state(&paths).await.unwrap(),
        ArchiveSchemaState::Incomplete {
            user_version: 1,
            required_objects: 2,
        }
    );
    assert!(read_archive_manifest(&paths).await.unwrap().is_empty());
    assert!(matches!(
        export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS).await,
        Err(StoreError::Archive {
            step: "cold read",
            ..
        })
    ));

    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&paths.db)
            .create_if_missing(false),
    )
    .await
    .expect("archive fixture should reopen read-write");
    sqlx::query("DROP TABLE metric_rollups_1h")
        .execute(&mut connection)
        .await
        .expect("fixture hourly table should drop");
    connection
        .close()
        .await
        .expect("archive fixture should close again");
    let settings = DashboardSettings {
        retention_ladder: ladder,
        ..DashboardSettings::default()
    };
    let coverage = store
        .history_coverage(&settings)
        .await
        .expect("coverage should stay lenient for every incomplete archive shape");
    assert_eq!(coverage.archive.queryable.bucket_count, 0);
    assert!(
        read_archive_points(&paths, i64::MIN, i64::MAX, 10)
            .await
            .expect("archive point reads should stay lenient for an incomplete v1 archive")
            .is_empty()
    );
}

#[tokio::test]
async fn archive_points_refuse_a_newer_user_version() {
    // Break caught: archive point reads serve a foreign/newer file that happens to have the table.
    let fixture = TempDatabase::new("archive-points-newer");
    let store = fixture.store().await;
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.archive.queryable = true;
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    seed_archive(&store, &paths, &[JAN_2023_MS]).await;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&paths.db)
            .create_if_missing(false),
    )
    .await
    .expect("archive fixture should open read-write");
    sqlx::query("PRAGMA user_version = 2")
        .execute(&mut connection)
        .await
        .expect("archive fixture should become a newer version");
    connection
        .close()
        .await
        .expect("archive fixture should close");

    assert!(matches!(
        read_archive_points(&paths, i64::MIN, i64::MAX, 10).await,
        Err(StoreError::Archive { step: "schema", .. })
    ));
    assert!(matches!(
        store.history_coverage(&settings).await,
        Err(StoreError::Archive { .. })
    ));
    assert!(matches!(
        read_archive_manifest(&paths).await,
        Err(StoreError::Archive { .. })
    ));
}

#[tokio::test]
async fn cold_export_writes_verified_month_files() {
    // Break caught: a pass omits an eligible month, publishes an unverifiable file, or advances twice.
    let mut fixture = TempDatabase::new("cold-verified-files");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    let starts = [
        JAN_2023_MS,
        JAN_2023_MS + HOUR_MS,
        FEB_2023_MS,
        FEB_2023_MS + HOUR_MS,
        MAR_2023_MS,
        MAR_2023_MS + HOUR_MS,
    ];
    seed_archive(&store, &paths, &starts).await;

    let written = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("all three old months should export in one pass");
    assert_eq!(
        store.attached_database_names().await.unwrap(),
        ["main".to_string()]
    );
    assert_eq!(
        written
            .iter()
            .map(|row| row.month.as_str())
            .collect::<Vec<_>>(),
        ["2023-01", "2023-02", "2023-03"]
    );
    for row in &written {
        let file = paths.directory.join(&row.file);
        let sidecar = paths.directory.join(format!("{}.sha256", row.file));
        assert!(file.is_file(), "{} should exist", file.display());
        assert!(sidecar.is_file(), "{} should exist", sidecar.display());
        assert_eq!(
            row.bytes as u64,
            std::fs::metadata(&file)
                .expect("cold file metadata should read")
                .len()
        );
        #[cfg(unix)]
        {
            let status = Command::new("sha256sum")
                .current_dir(&paths.directory)
                .args([
                    "-c",
                    sidecar
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                ])
                .status()
                .expect("sha256sum should run");
            assert!(status.success(), "sha256sum -c should verify {}", row.file);
        }
    }
    assert_eq!(read_archive_manifest(&paths).await.unwrap(), written);
    let settings = DashboardSettings {
        retention_ladder: ladder.clone(),
        ..DashboardSettings::default()
    };
    let coverage = store.history_coverage(&settings).await.unwrap();
    assert_eq!(coverage.archive.cold.file_count, 3);
    assert_eq!(
        coverage.archive.cold.bytes,
        written.iter().map(|row| row.bytes).sum::<i64>()
    );
    assert_eq!(
        store
            .history_state_get::<String>("coldExportedUntilMonth")
            .await
            .expect("cold watermark should read")
            .as_deref(),
        Some("2023-03")
    );
    assert!(
        export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
            .await
            .expect("second pass should succeed")
            .is_empty()
    );

    let artifact = paths.directory.join("tinytop-1h-2023-01.csv.gz");
    eprintln!("cold export acceptance artifact: {}", artifact.display());
    fixture.preserve();
}

#[tokio::test]
async fn corrupted_tmp_does_not_advance_month() {
    // Break caught: verification accepts a truncated gzip or a retry cannot replace it atomically.
    let fixture = TempDatabase::new("cold-corrupt-retry");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_archive(&store, &paths, &[JAN_2023_MS, JAN_2023_MS + HOUR_MS]).await;
    let first = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("fixture month should export");
    assert_eq!(first.len(), 1);
    let target = paths.directory.join(&first[0].file);
    let original_bytes = std::fs::metadata(&target).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&target)
        .expect("cold target should open for fixture corruption")
        .set_len(original_bytes / 2)
        .expect("fixture target should truncate");

    let error = verify_cold_file(
        &target,
        first[0].row_count,
        JAN_2023_MS,
        JAN_2023_MS + HOUR_MS,
    )
    .expect_err("truncated gzip must fail cold verification");
    assert!(matches!(
        error,
        StoreError::Archive {
            step: "cold verify",
            ..
        }
    ));

    let options = SqliteConnectOptions::new()
        .filename(&paths.db)
        .create_if_missing(false);
    let mut archive = SqliteConnection::connect_with(&options)
        .await
        .expect("archive fixture should open read-write");
    sqlx::query("DELETE FROM archive_manifest WHERE month = '2023-01'")
        .execute(&mut archive)
        .await
        .expect("fixture manifest row should delete");
    archive.close().await.expect("archive fixture should close");
    store
        .history_state_set("coldExportedUntilMonth", &"1900-01", AUG_29_2026_MS)
        .await
        .expect("fixture watermark should reset");

    let retried = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("retry should overwrite corrupt target atomically");
    assert_eq!(retried.len(), 1);
    assert!(std::fs::metadata(&target).unwrap().len() > original_bytes / 2);
    verify_cold_file(
        &target,
        retried[0].row_count,
        JAN_2023_MS,
        JAN_2023_MS + HOUR_MS,
    )
    .expect("re-exported file should verify");
}

#[tokio::test]
async fn corrupted_record_width_fails_verification() {
    // Break caught: verification accepts a data record wider than its DDL-derived header.
    let fixture = TempDatabase::new("cold-record-width");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_archive(&store, &paths, &[JAN_2023_MS]).await;
    let written = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("fixture month should export");
    let target = paths.directory.join(&written[0].file);
    let csv = read_csv_gz(&target);
    let mut records = csv
        .strip_suffix("\r\n")
        .expect("cold CSV should end in CRLF")
        .split("\r\n")
        .map(str::to_string)
        .collect::<Vec<_>>();
    records[1].push_str(",1");
    let corrupted = format!("{}\r\n", records.join("\r\n"));
    let file = std::fs::File::create(&target).expect("cold file should reopen for corruption");
    let mut encoder = GzEncoder::new(file, Compression::new(6));
    encoder
        .write_all(corrupted.as_bytes())
        .expect("corrupted CSV should recompress");
    encoder
        .finish()
        .expect("corrupted gzip should finish")
        .sync_all()
        .expect("corrupted gzip should sync");

    let error = verify_cold_file(&target, 1, JAN_2023_MS, JAN_2023_MS)
        .expect_err("a 20-field data record under a 19-field header must fail");
    assert!(matches!(
        &error,
        StoreError::Archive {
            step: "cold verify",
            ..
        }
    ));
    assert!(
        error
            .to_string()
            .contains("cold CSV data record 1 has 20 fields; expected 19 from header")
    );
}

#[test]
fn verify_rejects_a_quote_inside_an_unquoted_field() {
    // Break caught: the verifier treats a quote in an unquoted value as ordinary field data.
    let fixture = TempDatabase::new("cold-unquoted-quote");
    let path = fixture.dir.join("unquoted-quote.csv.gz");
    write_csv_gz(&path, &verifier_fixture_csv("1672531200000\""));

    let error = verify_cold_file(&path, 1, JAN_2023_MS, JAN_2023_MS)
        .expect_err("a quote inside an unquoted field must fail verification");
    assert!(matches!(
        &error,
        StoreError::Archive {
            step: "cold verify",
            ..
        }
    ));
    assert!(error.to_string().contains("record 2"));
}

#[test]
fn verify_rejects_junk_after_a_closing_quote() {
    // Break caught: the verifier accepts arbitrary bytes between a closing quote and delimiter.
    let fixture = TempDatabase::new("cold-post-quote-junk");
    let path = fixture.dir.join("post-quote-junk.csv.gz");
    write_csv_gz(&path, &verifier_fixture_csv("\"1672531200000\"x"));

    let error = verify_cold_file(&path, 1, JAN_2023_MS, JAN_2023_MS)
        .expect_err("junk after a closing quote must fail verification");
    assert!(matches!(
        &error,
        StoreError::Archive {
            step: "cold verify",
            ..
        }
    ));
    assert!(error.to_string().contains("record 2"));
}

#[tokio::test]
async fn csv_round_trip_is_row_exact() {
    // Break caught: CSV column order, numeric formatting, NULL encoding, or row order drifts.
    let fixture = TempDatabase::new("cold-row-exact");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    let first = bucket(JAN_2023_MS, 10.25);
    let mut second = bucket(JAN_2023_MS + HOUR_MS, 20.5);
    second.root_used = None;
    store.upsert_tier_bucket(Tier::L4, &first).await.unwrap();
    store.upsert_tier_bucket(Tier::L4, &second).await.unwrap();
    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 10).await.unwrap(),
        2
    );

    let written = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("fixture month should export");
    let csv = read_csv_gz(&paths.directory.join(&written[0].file));
    let records = parse_rfc4180(&csv);
    let archived = read_archive_points(&paths, i64::MIN, i64::MAX, 10)
        .await
        .expect("archive rows should read");
    assert_eq!(records.len(), archived.len() + 1);
    assert!(records.iter().all(|record| record.len() == 19));
    assert_eq!(
        records[0],
        [
            "bucket_start_ms",
            "first_captured_at_ms",
            "newest_captured_at_ms",
            "sample_count",
            "avg_cpu_usage_percent",
            "min_cpu_usage_percent",
            "max_cpu_usage_percent",
            "avg_memory_used_percent",
            "min_memory_used_percent",
            "max_memory_used_percent",
            "avg_swap_used_percent",
            "min_swap_used_percent",
            "max_swap_used_percent",
            "avg_load_percent",
            "min_load_percent",
            "max_load_percent",
            "avg_root_used_percent",
            "min_root_used_percent",
            "max_root_used_percent",
        ]
    );
    for (record, bucket) in records[1..].iter().zip(&archived) {
        let expected = bucket_fields(bucket);
        assert_eq!(&record[..4], &expected[..4]);
        for index in 4..19 {
            if expected[index].is_empty() {
                assert!(
                    record[index].is_empty(),
                    "column {index} should encode NULL empty"
                );
            } else {
                assert_eq!(
                    record[index].parse::<f64>().unwrap(),
                    expected[index].parse::<f64>().unwrap(),
                    "REAL column {index} should round-trip"
                );
            }
        }
    }
}

#[tokio::test]
async fn cold_export_skips_months_whose_hours_have_not_all_expired() {
    // Break caught: July exports on its first archived hour before its end plus L4 horizon and margin.
    let fixture = TempDatabase::new("cold-partial-month");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_archive(&store, &paths, &[JUL_2026_MS]).await;

    assert!(
        export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
            .await
            .expect("partial-month pass should succeed")
            .is_empty()
    );
    assert!(!paths.directory.join("tinytop-1h-2026-07.csv.gz").exists());
    assert_eq!(
        export_cold_months(&store, &paths, &ladder, SEP_2_2026_MS)
            .await
            .expect("month should export after every hour expires")
            .len(),
        1
    );
}

#[tokio::test]
async fn cold_export_never_deletes_archive_rows() {
    // Break caught: cold publication removes the queryable source rows.
    let fixture = TempDatabase::new("cold-never-deletes");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_archive(&store, &paths, &[JAN_2023_MS, FEB_2023_MS]).await;
    let before = archive_l4_count(&paths).await;

    export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("cold export should succeed");

    assert_eq!(archive_l4_count(&paths).await, before);
}

#[tokio::test]
async fn cold_export_waits_until_main_holds_no_rows_for_the_month() {
    // Break caught: a partially moved month is sealed while one of its rows remains in main.
    let fixture = TempDatabase::new("cold-waits-for-main");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_l4(&store, &[JAN_2023_MS, JAN_2023_MS + HOUR_MS]).await;
    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 1)
            .await
            .expect("first old bucket should move"),
        1
    );

    assert!(
        export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
            .await
            .expect("an incomplete month should wait normally")
            .is_empty()
    );
    assert!(!paths.directory.join("tinytop-1h-2023-01.csv.gz").exists());
    assert!(read_archive_manifest(&paths).await.unwrap().is_empty());
    assert_eq!(
        store
            .history_state_get::<String>("coldExportedUntilMonth")
            .await
            .expect("cold watermark should read"),
        None
    );

    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 1)
            .await
            .expect("second old bucket should move"),
        1
    );
    let written = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("fully moved month should export");
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].row_count, 2);
}

#[tokio::test]
async fn cold_export_stops_at_the_first_month_still_in_main() {
    // Break caught: a pass jumps the watermark beyond the first month that is still filling.
    let fixture = TempDatabase::new("cold-stops-at-incomplete");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    seed_l4(&store, &[JAN_2023_MS, FEB_2023_MS, FEB_2023_MS + HOUR_MS]).await;
    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 2)
            .await
            .expect("January and one February bucket should move"),
        2
    );

    let first = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("pass should stop normally at incomplete February");
    assert_eq!(
        first
            .iter()
            .map(|row| row.month.as_str())
            .collect::<Vec<_>>(),
        ["2023-01"]
    );
    assert_eq!(
        store
            .history_state_get::<String>("coldExportedUntilMonth")
            .await
            .unwrap()
            .as_deref(),
        Some("2023-01")
    );

    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 1)
            .await
            .expect("last February bucket should move"),
        1
    );
    let second = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("February should export on the next pass");
    assert_eq!(
        second
            .iter()
            .map(|row| (row.month.as_str(), row.row_count))
            .collect::<Vec<_>>(),
        [("2023-02", 2)]
    );
}

#[tokio::test]
async fn cold_export_exports_at_most_twelve_months_per_pass() {
    // Break caught: one scheduler/export-now call performs an unbounded number of file exports.
    let fixture = TempDatabase::new("cold-twelve-month-cap");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    let starts = [
        1_672_531_200_000,
        1_675_209_600_000,
        1_677_628_800_000,
        1_680_307_200_000,
        1_682_899_200_000,
        1_685_577_600_000,
        1_688_169_600_000,
        1_690_848_000_000,
        1_693_526_400_000,
        1_696_118_400_000,
        1_698_796_800_000,
        1_701_388_800_000,
        1_704_067_200_000,
    ];
    seed_archive(&store, &paths, &starts).await;

    let first = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("first bounded pass should succeed");
    assert_eq!(first.len(), 12);
    assert_eq!(first.last().map(|row| row.month.as_str()), Some("2023-12"));
    assert_eq!(read_archive_manifest(&paths).await.unwrap().len(), 12);
    assert_eq!(
        store
            .history_state_get::<String>("coldExportedUntilMonth")
            .await
            .unwrap()
            .as_deref(),
        Some("2023-12")
    );

    let second = export_cold_months(&store, &paths, &ladder, AUG_29_2026_MS)
        .await
        .expect("remaining month should export on the second pass");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].month, "2024-01");
    assert_eq!(read_archive_manifest(&paths).await.unwrap().len(), 13);
}

#[tokio::test]
async fn read_archive_manifest_never_creates_the_file() {
    // Break caught: read-only status inspection creates an empty archive sidecar.
    let fixture = TempDatabase::new("cold-manifest-no-create");
    let store = fixture.store().await;
    let ladder = cold_ladder();
    let paths = archive_paths(store.database_path(), &ladder.archive);
    assert!(!paths.db.exists());

    assert!(read_archive_manifest(&paths).await.unwrap().is_empty());

    assert!(!paths.db.exists());
}
