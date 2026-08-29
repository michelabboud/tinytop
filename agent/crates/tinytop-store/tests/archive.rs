use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{
    Connection, Row, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tinytop_store::{
    ArchiveErrorSource, DashboardSettings, HistoryPointMode, HistoryPointSource,
    HistoryPointsQuery, SqliteHistoryStore, StoreError,
    archive::{archive_paths, copy_expired_l4_batch, move_expired_l4, read_archive_points},
    ladder::{Stat, Tier, TierBucket},
    maintenance::maintain,
};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

struct TempDatabase {
    dir: PathBuf,
    url: String,
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
        Self { dir, url }
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
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
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
    // Break caught: post-delete bookkeeping errors falsely claim main was unchanged.
    let watermark = StoreError::Archive {
        step: "watermark",
        source: ArchiveErrorSource::Io(std::io::Error::other("fixture watermark failure")),
    }
    .to_string();
    assert!(watermark.contains(
        "remedy: the batch is committed in history-archive.sqlite and removed from the main database; only the watermark bookkeeping failed — retrying is safe, nothing is duplicated or lost"
    ));

    let insert = StoreError::Archive {
        step: "insert",
        source: ArchiveErrorSource::Io(std::io::Error::other("fixture insert failure")),
    }
    .to_string();
    assert!(insert.contains(
        "remedy: keep history-archive.sqlite and the main database unchanged, check the archive directory is writable, and retry — nothing was deleted from the main database"
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
                .contains("remedy: keep history-archive.sqlite and the main database unchanged")
        );
        assert!(std::error::Error::source(&error).is_some());
        match &error {
            StoreError::Archive { step, .. } => {
                assert!(
                    ["attach", "insert", "commit copy"].contains(&step),
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
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.archive.queryable = true;
    let relocated = fixture.dir.join("relocated");
    settings.retention_ladder.archive.directory = relocated.display().to_string();
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
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.archive.queryable = true;
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

    assert_eq!(
        move_expired_l4(&store, &paths, 2 * HOUR_MS, 10)
            .await
            .expect("archive row should move"),
        1
    );
    assert_eq!(
        move_expired_l4(&store, &paths, 2 * HOUR_MS, 10)
            .await
            .expect("same pooled connection should attach again cleanly"),
        0
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
