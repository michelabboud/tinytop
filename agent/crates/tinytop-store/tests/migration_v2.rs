use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use tinytop_store::migration::{CREATE_SCHEMA_V1_SQL, SCHEMA_VERSION, require_sqlite_at_least};
use tinytop_store::{SqliteHistoryStore, StoreError};

struct TempDatabase {
    dir: PathBuf,
    path: PathBuf,
    url: String,
}

impl TempDatabase {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should follow the Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-migration-v2-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("fixture directory should be created");
        let path = dir.join("history.sqlite");
        let url = format!("sqlite://{}", path.display());
        Self { dir, path, url }
    }

    async fn raw_pool(&self) -> SqlitePool {
        let options = SqliteConnectOptions::from_str(&self.url)
            .expect("fixture URL should parse")
            .create_if_missing(true);
        SqlitePool::connect_with(options)
            .await
            .expect("fixture pool should connect")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TableColumn {
    cid: i64,
    name: String,
    data_type: String,
    not_null: i64,
    default_value: Option<String>,
    primary_key: i64,
}

async fn process_sample_shape(pool: &SqlitePool) -> Vec<TableColumn> {
    sqlx::query("PRAGMA table_info(process_samples)")
        .fetch_all(pool)
        .await
        .expect("process_samples shape should read")
        .into_iter()
        .map(|row| TableColumn {
            cid: row.get("cid"),
            name: row.get("name"),
            data_type: row.get("type"),
            not_null: row.get("notnull"),
            default_value: row.get("dflt_value"),
            primary_key: row.get("pk"),
        })
        .collect()
}

async fn seed_v1_processes(fixture: &TempDatabase) {
    let pool = fixture.raw_pool().await;
    sqlx::raw_sql(CREATE_SCHEMA_V1_SQL)
        .execute(&pool)
        .await
        .expect("the authentic v1 DDL should apply");
    for (captured_at_ms, rank, pid, command) in [
        (1_000_i64, 1_i64, 10_i64, "alpha --one"),
        (1_000, 2, 11, "beta --two"),
        (2_000, 1, 12, "alpha --one"),
        (2_000, 2, 13, "gamma --three"),
        (2_000, 3, 14, "beta --two"),
    ] {
        sqlx::query(
            "INSERT INTO process_samples (captured_at_ms, rank, pid, command, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at) VALUES (?, ?, ?, ?, 1.0, 2.0, 3, NULL, NULL)",
        )
        .bind(captured_at_ms)
        .bind(rank)
        .bind(pid)
        .bind(command)
        .execute(&pool)
        .await
        .expect("v1 process row should insert");
    }
    pool.close().await;
}

async fn marker_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated' AND label = 'SQLite schema migrated from v1 to v2'",
    )
    .fetch_one(pool)
    .await
    .expect("migration marker count should read")
}

#[tokio::test]
async fn fresh_database_at_v4_keeps_the_v2_process_dictionary() {
    let fixture = TempDatabase::new("fresh");
    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh database should connect");
    assert_eq!(store.user_version().await.expect("version should read"), 4);
    store.close().await.expect("store should close");

    let pool = fixture.raw_pool().await;
    let columns = process_sample_shape(&pool).await;
    assert!(columns.iter().any(|column| column.name == "command_id"));
    assert!(!columns.iter().any(|column| column.name == "command"));
    for table in ["process_commands", "process_samples_fast"] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("table existence should read");
        assert_eq!(exists, 1, "missing table {table}");
    }
    let fast_indexes: Vec<String> = sqlx::query("PRAGMA index_list(process_samples_fast)")
        .fetch_all(&pool)
        .await
        .expect("fast indexes should read")
        .into_iter()
        .map(|row| row.get("name"))
        .collect();
    assert!(
        fast_indexes
            .iter()
            .any(|name| name == "idx_process_samples_fast_command")
    );
    pool.close().await;
}

#[tokio::test]
async fn v1_fixture_with_three_commands_migrates_through_v2_to_v4() {
    let fixture = TempDatabase::new("three-commands");
    seed_v1_processes(&fixture).await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v1 database should migrate");
    assert_eq!(store.user_version().await.expect("version should read"), 4);
    store.close().await.expect("store should close");

    let pool = fixture.raw_pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM process_commands")
            .fetch_one(&pool)
            .await
            .expect("command count should read"),
        3
    );
    let columns = process_sample_shape(&pool).await;
    assert!(!columns.iter().any(|column| column.name == "command"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM process_samples WHERE command_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("NULL command count should read"),
        0
    );
    let joined: Vec<String> = sqlx::query_scalar(
        "SELECT c.command FROM process_samples p JOIN process_commands c ON c.command_id = p.command_id ORDER BY p.captured_at_ms, p.rank",
    )
    .fetch_all(&pool)
    .await
    .expect("joined commands should read");
    assert_eq!(
        joined,
        [
            "alpha --one",
            "beta --two",
            "alpha --one",
            "gamma --three",
            "beta --two",
        ]
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .expect("integrity check should run"),
        "ok"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .expect("foreign key check should run")
            .is_empty()
    );
    assert_eq!(marker_count(&pool).await, 1);
    pool.close().await;

    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v2 database should reconnect")
        .close()
        .await
        .expect("store should close");
    let pool = fixture.raw_pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&pool)
            .await
            .expect("version should read"),
        4
    );
    assert_eq!(marker_count(&pool).await, 1);
    pool.close().await;
}

#[tokio::test]
async fn v1_fixture_with_an_index_on_command_refuses_and_leaves_the_file_untouched() {
    let fixture = TempDatabase::new("indexed-command");
    seed_v1_processes(&fixture).await;

    let pool = fixture.raw_pool().await;
    sqlx::query("CREATE INDEX idx_probe_command ON process_samples (command)")
        .execute(&pool)
        .await
        .expect("probe index should be created");
    pool.close().await;

    let error = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect_err("the command index should prevent DROP COLUMN");
    match error {
        StoreError::Migration { reason, remedy } => {
            assert!(
                reason.contains("DROP COLUMN command failed"),
                "migration reason should identify the failed DROP COLUMN: {reason}"
            );
            assert!(
                reason.contains("idx_probe_command"),
                "migration reason should retain SQLite's index name: {reason}"
            );
            assert!(
                reason.contains("after drop column"),
                "migration reason should retain SQLite's DROP COLUMN diagnostic: {reason}"
            );
            assert!(
                reason.contains("no such column: command"),
                "migration reason should retain SQLite's missing-column diagnostic: {reason}"
            );
            assert!(remedy.contains("database was not modified"));
        }
        other => panic!("expected migration refusal, observed {other:?}"),
    }

    let pool = fixture.raw_pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&pool)
            .await
            .expect("version should read"),
        1
    );
    let columns = process_sample_shape(&pool).await;
    assert!(columns.iter().any(|column| column.name == "command"));
    assert!(!columns.iter().any(|column| column.name == "command_id"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('process_commands', 'process_samples_fast')",
        )
        .fetch_one(&pool)
        .await
        .expect("v2 table count should read"),
        0
    );
    assert_eq!(marker_count(&pool).await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM process_samples")
            .fetch_one(&pool)
            .await
            .expect("process row count should read"),
        5
    );
    sqlx::query("DROP INDEX idx_probe_command")
        .execute(&pool)
        .await
        .expect("probe index should be removed");
    pool.close().await;

    let store = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("migration should succeed after removing the probe index");
    assert_eq!(store.user_version().await.expect("version should read"), 4);
    store.close().await.expect("store should close");
    let pool = fixture.raw_pool().await;
    assert_eq!(marker_count(&pool).await, 1);
    pool.close().await;
}

#[tokio::test]
async fn migrated_and_fresh_process_samples_have_identical_shape() {
    let fresh = TempDatabase::new("shape-fresh");
    SqliteHistoryStore::connect(&fresh.url)
        .await
        .expect("fresh database should connect")
        .close()
        .await
        .expect("fresh store should close");
    let fresh_pool = fresh.raw_pool().await;
    let fresh_shape = process_sample_shape(&fresh_pool).await;
    fresh_pool.close().await;

    let migrated = TempDatabase::new("shape-migrated");
    seed_v1_processes(&migrated).await;
    SqliteHistoryStore::connect(&migrated.url)
        .await
        .expect("v1 database should migrate")
        .close()
        .await
        .expect("migrated store should close");
    let migrated_pool = migrated.raw_pool().await;
    let migrated_shape = process_sample_shape(&migrated_pool).await;
    migrated_pool.close().await;

    assert_eq!(fresh_shape, migrated_shape);
}

#[tokio::test]
async fn sqlite_version_requirement_is_checked() {
    let error =
        require_sqlite_at_least("3.34.1", (3, 35, 0)).expect_err("SQLite 3.34.1 should be refused");
    match error {
        StoreError::Migration { reason, .. } => assert_eq!(
            reason,
            "schema migration requires SQLite ≥ 3.35.0 (linked: 3.34.1)"
        ),
        other => panic!("expected migration refusal, observed {other:?}"),
    }
    require_sqlite_at_least("3.35.0", (3, 35, 0)).expect("minimum should pass");
    require_sqlite_at_least("3.51.3", (3, 35, 0)).expect("newer version should pass");

    let fixture = TempDatabase::new("linked-version");
    let pool = fixture.raw_pool().await;
    let linked: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await
        .expect("linked SQLite version should read");
    eprintln!("linked SQLite version: {linked}");
    require_sqlite_at_least(&linked, (3, 35, 0)).expect("linked SQLite should pass");
    pool.close().await;
}

#[tokio::test]
async fn newer_schema_version_is_refused() {
    let fixture = TempDatabase::new("newer-version");
    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh database should connect")
        .close()
        .await
        .expect("fresh database should close");
    set_sqlite_user_version(&fixture.path, (SCHEMA_VERSION + 1) as u32);

    let error = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect_err("newer schema should be refused")
        .to_string();
    assert!(error.contains("unsupported SQLite schema version 5"));
    assert!(error.contains("supported version is 4"));
}

fn set_sqlite_user_version(path: &Path, user_version: u32) {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "fixture sidecar {} should be removable: {error}",
                sidecar.display()
            ),
        }
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("SQLite fixture should open for user_version update");
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)
        .expect("SQLite fixture should have a complete header");
    assert_eq!(&header[..16], b"SQLite format 3\0");
    file.seek(SeekFrom::Start(60))
        .expect("SQLite user_version header offset should be seekable");
    file.write_all(&user_version.to_be_bytes())
        .expect("SQLite user_version header should be writable");
    file.sync_all()
        .expect("SQLite user_version update should be durable");
}
