use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value as JsonValue, json};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use tinytop_store::migration::CREATE_SCHEMA_V2_SQL;
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
            "tinytop-migration-v3-{label}-{}-{stamp}",
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

#[derive(Debug, PartialEq)]
struct MigratedMetricRow {
    sample_id: i64,
    identity_id: Option<i64>,
    uptime_seconds: Option<i64>,
    memory_available_bytes: Option<i64>,
    swap_free_bytes: Option<i64>,
    last_pid: Option<i64>,
    filesystems_captured_at_ms: Option<i64>,
}

type TableInfoRow = (i64, String, String, i64, Option<String>, i64);

async fn table_info(pool: &SqlitePool, table: &str) -> Vec<TableInfoRow> {
    let sql = match table {
        "metric_samples" => "PRAGMA table_info(metric_samples)",
        "host_identity" => "PRAGMA table_info(host_identity)",
        "fs_mount_events" => "PRAGMA table_info(fs_mount_events)",
        "fs_samples" => "PRAGMA table_info(fs_samples)",
        "process_samples" => "PRAGMA table_info(process_samples)",
        "process_samples_fast" => "PRAGMA table_info(process_samples_fast)",
        "process_commands" => "PRAGMA table_info(process_commands)",
        other => panic!("unsupported table_info fixture table {other}"),
    };
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|error| panic!("{table} table_info should read: {error}"))
        .into_iter()
        .map(|row| {
            (
                row.get("cid"),
                row.get("name"),
                row.get("type"),
                row.get("notnull"),
                row.get("dflt_value"),
                row.get("pk"),
            )
        })
        .collect()
}

async fn index_names(pool: &SqlitePool, table: &str) -> Vec<String> {
    let sql = match table {
        "metric_samples" => "PRAGMA index_list(metric_samples)",
        "fs_samples" => "PRAGMA index_list(fs_samples)",
        other => panic!("unsupported index_list fixture table {other}"),
    };
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|error| panic!("{table} index_list should read: {error}"))
        .into_iter()
        .map(|row| row.get("name"))
        .collect()
}

#[tokio::test]
async fn v2_fixture_with_json_rows_migrates_to_v3() {
    // Break caught: the v2 rebuild loses sample ids, borrows scalar values from
    // neighbouring JSON rows, fails to intern exact identities, or invents an
    // identity for a row whose JSON had already been stripped.
    let fixture = TempDatabase::new("json-backfill");
    let pool = seed_v2_schema(&fixture).await;
    for (sample_id, captured_at_ms, legacy_json) in [
        (
            11_i64,
            1_000_i64,
            legacy_snapshot("kernel-a", 101, 601, 701, Some(801), Some(901), json!([])),
        ),
        (
            12,
            2_000,
            legacy_snapshot("kernel-a", 102, 602, 702, None, None, json!([])),
        ),
        (
            13,
            3_000,
            legacy_snapshot("kernel-b", 103, 603, 703, Some(803), Some(903), json!([])),
        ),
        (14, 4_000, None),
        (15, 5_000, None),
    ] {
        insert_v2_metric(&pool, sample_id, captured_at_ms, legacy_json.as_deref()).await;
    }
    for (captured_at_ms, mount) in [(10_i64, "/"), (10, "/data"), (20, "/")] {
        insert_filesystem(&pool, captured_at_ms, mount).await;
    }
    pool.close().await;

    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("v2 database should migrate")
        .close()
        .await
        .expect("migrated store should close");

    let pool = fixture.raw_pool().await;
    assert_eq!(user_version(&pool).await, 3);

    let columns = sqlx::query("PRAGMA table_info(metric_samples)")
        .fetch_all(&pool)
        .await
        .expect("metric_samples shape should read");
    assert!(
        !columns
            .iter()
            .any(|row| row.get::<String, _>("name") == "snapshot_json")
    );
    for expected in [
        "identity_id",
        "uptime_seconds",
        "memory_available_bytes",
        "swap_free_bytes",
        "last_pid",
        "filesystems_captured_at_ms",
    ] {
        assert!(
            columns
                .iter()
                .any(|row| row.get::<String, _>("name") == expected),
            "missing v3 column {expected}"
        );
    }
    for nullable in ["runnable_threads", "total_threads"] {
        let column = columns
            .iter()
            .find(|row| row.get::<String, _>("name") == nullable)
            .expect("nullable thread column should exist");
        assert_eq!(column.get::<i64, _>("notnull"), 0, "{nullable}");
    }

    let rows = sqlx::query(
        r#"
        SELECT sample_id, identity_id, uptime_seconds, memory_available_bytes,
               swap_free_bytes, last_pid, filesystems_captured_at_ms
        FROM metric_samples
        ORDER BY sample_id
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("migrated metric rows should read")
    .into_iter()
    .map(|row| MigratedMetricRow {
        sample_id: row.get("sample_id"),
        identity_id: row.get("identity_id"),
        uptime_seconds: row.get("uptime_seconds"),
        memory_available_bytes: row.get("memory_available_bytes"),
        swap_free_bytes: row.get("swap_free_bytes"),
        last_pid: row.get("last_pid"),
        filesystems_captured_at_ms: row.get("filesystems_captured_at_ms"),
    })
    .collect::<Vec<_>>();
    assert_eq!(
        rows.iter().map(|row| row.sample_id).collect::<Vec<_>>(),
        [11, 12, 13, 14, 15]
    );
    let identity_a = rows[0].identity_id.expect("row 11 should be assembleable");
    assert_eq!(rows[1].identity_id, Some(identity_a));
    let identity_b = rows[2].identity_id.expect("row 13 should be assembleable");
    assert_ne!(identity_a, identity_b);
    assert_eq!(
        rows[0],
        MigratedMetricRow {
            sample_id: 11,
            identity_id: Some(identity_a),
            uptime_seconds: Some(101),
            memory_available_bytes: Some(601),
            swap_free_bytes: Some(701),
            last_pid: Some(801),
            filesystems_captured_at_ms: Some(901),
        }
    );
    assert_eq!(
        rows[1],
        MigratedMetricRow {
            sample_id: 12,
            identity_id: Some(identity_a),
            uptime_seconds: Some(102),
            memory_available_bytes: Some(602),
            swap_free_bytes: Some(702),
            last_pid: None,
            filesystems_captured_at_ms: None,
        }
    );
    assert_eq!(
        rows[2],
        MigratedMetricRow {
            sample_id: 13,
            identity_id: Some(identity_b),
            uptime_seconds: Some(103),
            memory_available_bytes: Some(603),
            swap_free_bytes: Some(703),
            last_pid: Some(803),
            filesystems_captured_at_ms: Some(903),
        }
    );
    for stripped in &rows[3..] {
        assert_eq!(
            *stripped,
            MigratedMetricRow {
                sample_id: stripped.sample_id,
                identity_id: None,
                uptime_seconds: None,
                memory_available_bytes: None,
                swap_free_bytes: None,
                last_pid: None,
                filesystems_captured_at_ms: None,
            }
        );
    }

    let identities = sqlx::query(
        r#"
        SELECT first_seen_ms, hostname, platform, arch, distro, kernel,
               runtime_kind, runtime_confidence, runtime_reason
        FROM host_identity
        ORDER BY first_seen_ms
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("interned identities should read");
    assert_eq!(identities.len(), 2);
    for (index, expected_kernel, expected_first_seen) in
        [(0_usize, "kernel-a", 1_000_i64), (1, "kernel-b", 3_000)]
    {
        let row = &identities[index];
        assert_eq!(row.get::<i64, _>("first_seen_ms"), expected_first_seen);
        assert_eq!(row.get::<String, _>("hostname"), "fixture-host");
        assert_eq!(row.get::<String, _>("platform"), "linux");
        assert_eq!(row.get::<String, _>("arch"), "x86_64");
        assert_eq!(row.get::<String, _>("distro"), "Fixture Linux");
        assert_eq!(row.get::<String, _>("kernel"), expected_kernel);
        assert_eq!(row.get::<String, _>("runtime_kind"), "Linux");
        assert_eq!(row.get::<String, _>("runtime_confidence"), "high");
        assert_eq!(row.get::<String, _>("runtime_reason"), "fixture runtime");
    }

    let events = sqlx::query(
        "SELECT captured_at_ms, mount, present FROM fs_mount_events ORDER BY captured_at_ms, mount",
    )
    .fetch_all(&pool)
    .await
    .expect("filesystem presence events should read")
    .into_iter()
    .map(|row| {
        (
            row.get::<i64, _>("captured_at_ms"),
            row.get::<String, _>("mount"),
            row.get::<i64, _>("present"),
        )
    })
    .collect::<Vec<_>>();
    assert_eq!(
        events,
        [
            (10, "/".to_string(), 1),
            (10, "/data".to_string(), 1),
            (11, "/data".to_string(), 0),
        ]
    );

    let markers: Vec<String> = sqlx::query_scalar(
        "SELECT details_json FROM app_events WHERE marker_type = 'schemaMigrated' AND label = 'SQLite schema migrated from v2 to v3'",
    )
    .fetch_all(&pool)
    .await
    .expect("v3 marker should read");
    assert_eq!(markers.len(), 1);
    let marker: JsonValue = serde_json::from_str(&markers[0]).expect("v3 marker should be JSON");
    assert_eq!(marker["fromVersion"], 2);
    assert_eq!(marker["toVersion"], 3);
    assert_eq!(marker["sampleRows"], 5);
    assert_eq!(marker["jsonRowsDecoded"], 3);
    assert_eq!(marker["identitiesInterned"], 2);
    assert_eq!(marker["eventsWritten"], 3);
    assert!(marker["durationMs"].as_i64().is_some());
    pool.close().await;
}

#[tokio::test]
async fn fresh_database_is_created_at_v3() {
    // Break caught: fresh files run an older DDL and acquire a migration marker
    // despite never having contained an older schema.
    let fixture = TempDatabase::new("fresh");
    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("fresh database should connect")
        .close()
        .await
        .expect("fresh store should close");

    let pool = fixture.raw_pool().await;
    assert_eq!(user_version(&pool).await, 3);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated'",
        )
        .fetch_one(&pool)
        .await
        .expect("fresh marker count should read"),
        0
    );
    for table in ["host_identity", "fs_mount_events"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("fresh v3 table count should read"),
            1,
            "missing fresh v3 table {table}"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .expect("fresh integrity check should run"),
        "ok"
    );
    pool.close().await;
}

#[tokio::test]
async fn migrated_and_fresh_v3_schemas_are_identical() {
    // Break caught: fresh-v3 DDL and the v2-to-v3 rebuild drift in any column
    // attribute or required index while both still report user_version 3.
    let migrated_fixture = TempDatabase::new("migrated-schema");
    let migrated_seed = seed_v2_schema(&migrated_fixture).await;
    insert_v2_metric(
        &migrated_seed,
        21,
        1_000,
        legacy_snapshot("kernel-a", 121, 621, 721, Some(821), Some(1_000), json!([])).as_deref(),
    )
    .await;
    insert_filesystem(&migrated_seed, 1_000, "/").await;
    migrated_seed.close().await;
    SqliteHistoryStore::connect(&migrated_fixture.url)
        .await
        .expect("v2 fixture should migrate")
        .close()
        .await
        .expect("migrated store should close");

    let fresh_fixture = TempDatabase::new("fresh-schema");
    SqliteHistoryStore::connect(&fresh_fixture.url)
        .await
        .expect("fresh v3 database should connect")
        .close()
        .await
        .expect("fresh store should close");

    let migrated_pool = migrated_fixture.raw_pool().await;
    let fresh_pool = fresh_fixture.raw_pool().await;
    for table in [
        "metric_samples",
        "host_identity",
        "fs_mount_events",
        "fs_samples",
        "process_samples",
        "process_samples_fast",
        "process_commands",
    ] {
        let migrated_rows = table_info(&migrated_pool, table).await;
        let fresh_rows = table_info(&fresh_pool, table).await;
        let first_difference = migrated_rows
            .iter()
            .zip(&fresh_rows)
            .position(|(migrated, fresh)| migrated != fresh)
            .unwrap_or_else(|| migrated_rows.len().min(fresh_rows.len()));
        assert_eq!(
            migrated_rows,
            fresh_rows,
            "schema mismatch for {table}; first differing row {first_difference}: migrated {:?}, fresh {:?}",
            migrated_rows.get(first_difference),
            fresh_rows.get(first_difference),
        );
    }

    let migrated_metric_indexes = index_names(&migrated_pool, "metric_samples").await;
    let fresh_metric_indexes = index_names(&fresh_pool, "metric_samples").await;
    assert_eq!(
        migrated_metric_indexes, fresh_metric_indexes,
        "metric_samples index names differ"
    );
    assert_eq!(
        fresh_metric_indexes,
        [
            "idx_metric_samples_runtime_captured_at",
            "idx_metric_samples_captured_at",
            "sqlite_autoindex_metric_samples_1",
        ]
    );

    let migrated_fs_indexes = index_names(&migrated_pool, "fs_samples").await;
    let fresh_fs_indexes = index_names(&fresh_pool, "fs_samples").await;
    assert_eq!(
        migrated_fs_indexes, fresh_fs_indexes,
        "fs_samples index names differ"
    );
    assert_eq!(
        fresh_fs_indexes,
        ["idx_fs_samples_mount_time", "sqlite_autoindex_fs_samples_1"]
    );
    migrated_pool.close().await;
    fresh_pool.close().await;
}

#[tokio::test]
async fn v2_fixture_with_legacy_negative_inode_counts_migrates_and_counts() {
    // Break caught: the v3 migration decodes a legacy Bun filesystem payload
    // directly into the strict Rust type and refuses the whole database when
    // inodeTotal - inodeFree produced a negative inodeUsed value.
    let fixture = TempDatabase::new("legacy-negative-inodes");
    let pool = seed_v2_schema(&fixture).await;
    let filesystem_with_negative_inodes = json!([{
        "filesystem": "drivers",
        "type": "9p",
        "sizeBytes": 1,
        "usedBytes": 1,
        "availableBytes": 0,
        "usedPercent": 100.0,
        "mount": "/usr/lib/wsl/drivers",
        "inodeUsedPercent": null,
        "inodeUsed": -999001,
        "inodeTotal": 999,
    }]);
    let healthy_filesystem = json!([{
        "filesystem": "drivers",
        "type": "9p",
        "sizeBytes": 1,
        "usedBytes": 1,
        "availableBytes": 0,
        "usedPercent": 100.0,
        "mount": "/usr/lib/wsl/drivers",
        "inodeUsedPercent": 0.1,
        "inodeUsed": 1,
        "inodeTotal": 999,
    }]);
    insert_v2_metric(
        &pool,
        41,
        1_000,
        legacy_snapshot(
            "kernel-a",
            141,
            641,
            741,
            Some(841),
            Some(1_000),
            filesystem_with_negative_inodes,
        )
        .as_deref(),
    )
    .await;
    insert_v2_metric(
        &pool,
        42,
        2_000,
        legacy_snapshot(
            "kernel-a",
            142,
            642,
            742,
            Some(842),
            Some(1_000),
            healthy_filesystem,
        )
        .as_deref(),
    )
    .await;
    insert_filesystem(&pool, 1_000, "/usr/lib/wsl/drivers").await;
    pool.close().await;

    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("known legacy negative inode counts should be normalised")
        .close()
        .await
        .expect("migrated store should close");

    let pool = fixture.raw_pool().await;
    assert_eq!(user_version(&pool).await, 3);
    let identities = sqlx::query(
        "SELECT sample_id, identity_id FROM metric_samples WHERE sample_id IN (41, 42) ORDER BY sample_id",
    )
    .fetch_all(&pool)
    .await
    .expect("migrated identities should read");
    assert_eq!(identities.len(), 2);
    for row in identities {
        assert!(
            row.get::<Option<i64>, _>("identity_id").is_some(),
            "sample {} should be assembleable",
            row.get::<i64, _>("sample_id")
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fs_samples")
            .fetch_one(&pool)
            .await
            .expect("filesystem sample count should read"),
        1
    );

    let marker: String = sqlx::query_scalar(
        "SELECT details_json FROM app_events WHERE marker_type = 'schemaMigrated' AND label = 'SQLite schema migrated from v2 to v3'",
    )
    .fetch_one(&pool)
    .await
    .expect("v3 migration audit should read");
    let marker: JsonValue = serde_json::from_str(&marker).expect("v3 marker should be JSON");
    assert_eq!(marker["legacyInodeRowsNormalised"], 1);
    assert_eq!(marker["jsonRowsDecoded"], 2);
    pool.close().await;
}

#[tokio::test]
async fn v2_fixture_with_undecodable_json_refuses_and_leaves_the_file_untouched() {
    // Break caught: malformed surviving JSON is silently discarded after the
    // old table is dropped, or a refusal leaks partial v3 tables/markers.
    let fixture = TempDatabase::new("undecodable");
    let pool = seed_v2_schema(&fixture).await;
    insert_v2_metric(&pool, 41, 1_000, Some(r#"{"not":"a snapshot"}"#)).await;
    pool.close().await;

    let error = SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect_err("undecodable JSON must refuse schema v3 migration");
    match error {
        StoreError::Migration { reason, remedy } => {
            assert!(reason.contains("metric_samples row 41"), "{reason}");
            assert!(reason.contains("does not decode"), "{reason}");
            assert!(remedy.contains("snapshot_json = NULL"), "{remedy}");
            assert!(remedy.contains("database was not modified"), "{remedy}");
        }
        other => panic!("expected migration refusal, observed {other:?}"),
    }

    let pool = fixture.raw_pool().await;
    assert_eq!(user_version(&pool).await, 2);
    let columns = sqlx::query("PRAGMA table_info(metric_samples)")
        .fetch_all(&pool)
        .await
        .expect("metric_samples shape should read");
    assert!(
        columns
            .iter()
            .any(|row| row.get::<String, _>("name") == "snapshot_json")
    );
    for table in ["host_identity", "fs_mount_events", "metric_samples_v3"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("rolled-back table count should read"),
            0,
            "transaction leaked table {table}"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM app_events WHERE marker_type = 'schemaMigrated'",
        )
        .fetch_one(&pool)
        .await
        .expect("rolled-back marker count should read"),
        0
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "needs TINYTOP_V1_FIXTURE=<path to a v1 history.sqlite>"]
async fn real_v1_file_copy_migrates_to_v3() {
    let source = std::env::var("TINYTOP_V1_FIXTURE").unwrap_or_else(|error| {
        panic!("TINYTOP_V1_FIXTURE must name a readable v1 database: {error}")
    });
    let source_path = PathBuf::from(source);
    assert!(
        source_path.is_file(),
        "TINYTOP_V1_FIXTURE must name a readable v1 database"
    );
    let fixture = TempDatabase::new("real-v1-copy");
    fs::copy(&source_path, &fixture.path)
        .unwrap_or_else(|error| panic!("TINYTOP_V1_FIXTURE copy should succeed: {error}"));

    let started = Instant::now();
    SqliteHistoryStore::connect(&fixture.url)
        .await
        .expect("real v1 copy should migrate")
        .close()
        .await
        .expect("migrated real-file store should close");
    let total_elapsed_ms = started.elapsed().as_millis();

    let pool = fixture.raw_pool().await;
    assert_eq!(user_version(&pool).await, 3);
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .expect("real-file integrity check should run"),
        "ok"
    );
    let migration_durations = sqlx::query(
        r#"
        SELECT json_extract(details_json, '$.fromVersion') AS from_version,
               json_extract(details_json, '$.toVersion') AS to_version,
               json_extract(details_json, '$.durationMs') AS duration_ms
        FROM app_events
        WHERE marker_type = 'schemaMigrated'
          AND json_extract(details_json, '$.fromVersion') IN (1, 2)
        ORDER BY from_version
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("real-file migration timings should read");
    for row in migration_durations {
        println!(
            "migration v{} -> v{} took {} ms",
            row.get::<i64, _>("from_version"),
            row.get::<i64, _>("to_version"),
            row.get::<i64, _>("duration_ms")
        );
    }
    println!("migration v1 -> v3 total took {total_elapsed_ms} ms");
    pool.close().await;
}

async fn seed_v2_schema(fixture: &TempDatabase) -> SqlitePool {
    let pool = fixture.raw_pool().await;
    for group in CREATE_SCHEMA_V2_SQL {
        sqlx::raw_sql(group)
            .execute(&pool)
            .await
            .expect("authentic v2 DDL group should apply");
    }
    assert_eq!(user_version(&pool).await, 2);
    pool
}

async fn insert_v2_metric(
    pool: &SqlitePool,
    sample_id: i64,
    captured_at_ms: i64,
    legacy_json: Option<&str>,
) {
    sqlx::query(
        r#"
        INSERT INTO metric_samples (
          sample_id, captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
          cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
          memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
          load_one, load_five, load_fifteen, load_percent, runnable_threads,
          total_threads, root_used_percent, snapshot_json
        ) VALUES (?, ?, '2026-08-30T00:00:01Z', 'fixture-host', 'Linux',
                  1.0, 4, 2.0, 2, 100, 3.0, 3, 100,
                  0.1, 0.2, 0.3, 4.0, 1, 2, NULL, ?)
        "#,
    )
    .bind(sample_id)
    .bind(captured_at_ms)
    .bind(legacy_json)
    .execute(pool)
    .await
    .expect("v2 metric row should seed");
}

async fn insert_filesystem(pool: &SqlitePool, captured_at_ms: i64, mount: &str) {
    sqlx::query(
        r#"
        INSERT INTO fs_samples (
          captured_at_ms, mount, filesystem, fs_type, size_bytes, used_bytes,
          available_bytes, used_percent, inode_used_percent, inode_used, inode_total
        ) VALUES (?, ?, '/dev/fixture', 'ext4', 100, 25, 75, 25.0, 10.0, 1, 10)
        "#,
    )
    .bind(captured_at_ms)
    .bind(mount)
    .execute(pool)
    .await
    .expect("v2 filesystem row should seed");
}

fn legacy_snapshot(
    kernel: &str,
    uptime_seconds: i64,
    memory_available_bytes: i64,
    swap_free_bytes: i64,
    last_pid: Option<i64>,
    filesystems_captured_at_ms: Option<i64>,
    filesystems: JsonValue,
) -> Option<String> {
    let mut load = json!({
        "one": 0.1,
        "five": 0.2,
        "fifteen": 0.3,
        "runnable": 1,
        "totalThreads": 2,
    });
    if let Some(last_pid) = last_pid {
        load["lastPid"] = json!(last_pid);
    }
    let mut snapshot = json!({
        "timestamp": "2026-08-30T00:00:01Z",
        "identity": {
            "hostname": "fixture-host",
            "platform": "linux",
            "arch": "x86_64",
            "distro": "Fixture Linux",
            "kernel": kernel,
            "runtime": {
                "kind": "Linux",
                "confidence": "high",
                "reason": "fixture runtime",
            },
            "uptimeSeconds": uptime_seconds,
        },
        "cpu": { "usagePercent": 1.0, "cores": 4 },
        "memory": {
            "totalBytes": 1000,
            "availableBytes": memory_available_bytes,
            "usedBytes": 400,
            "usedPercent": 40.0,
        },
        "swap": {
            "totalBytes": 1000,
            "freeBytes": swap_free_bytes,
            "usedBytes": 300,
            "usedPercent": 30.0,
        },
        "load": load,
        "pressure": { "cpu": {}, "memory": {}, "io": {} },
        "filesystems": filesystems,
        "processes": [],
    });
    if let Some(stamp) = filesystems_captured_at_ms {
        snapshot["filesystemsCapturedAtMs"] = json!(stamp);
    }
    Some(serde_json::to_string(&snapshot).expect("fixture snapshot should serialize"))
}

async fn user_version(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await
        .expect("schema version should read")
}
