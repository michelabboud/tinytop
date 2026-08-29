use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};
use tinytop_store::{
    DashboardSettings, SqliteHistoryStore, SysinfoFreeBytes,
    archive::{archive_paths, move_expired_l4},
    check_disk,
    ladder::{Stat, Tier, TierBucket},
};

const HOUR_MS: i64 = 3_600_000;
const JAN_2023_MS: i64 = 1_672_531_200_000;

struct TempDatabase {
    dir: PathBuf,
    db_path: PathBuf,
    database_url: String,
    pre_image_path: PathBuf,
}

impl TempDatabase {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-cli-db-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("CLI database fixture directory should be created");
        let db_path = dir.join("h.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let pre_image_path = PathBuf::from(format!("{}.pre-v0.sqlite", db_path.display()));
        Self {
            dir,
            db_path,
            database_url,
            pre_image_path,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        run_database_command(&self.database_url, args)
    }

    fn run_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"));
        command.args(args).args(["--sqlite", &self.database_url]);
        for (key, value) in env {
            command.env(key, value);
        }
        command
            .output()
            .expect("tinytop-agent database command should run")
    }

    fn initialize_v2(&self) {
        // Inspection commands deliberately refuse missing databases; collection owns creation.
        let output = self.run(&["collect", "--json"]);
        assert_success(&output);
    }

    fn initialize_populated_v2(&self) {
        let output = self.run(&["collect", "--json"]);
        assert_success(&output);
    }
}

fn run_database_command(database_url: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args(args)
        .args(["--sqlite", database_url])
        .output()
        .expect("tinytop-agent database command should run")
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should be JSON ({error})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn key_set(object: &Map<String, Value>) -> BTreeSet<&str> {
    object.keys().map(String::as_str).collect()
}

fn set_sqlite_user_version(path: &Path, user_version: u32) {
    // Before explicit CLI close, the agent's WAL mode could leave page 1 in a
    // fixture WAL and override the main-file header below. Sidecar cleanup stays
    // as defensive fixture isolation and should now be a no-op. These are the
    // two exact sidecars owned here.
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
        .expect("SQLite user_version fixture update should be durable");
}

fn sqlite_user_version(path: &Path) -> u32 {
    let mut file = File::open(path).expect("SQLite fixture should open for user_version read");
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)
        .expect("SQLite fixture should have a complete header");
    assert_eq!(&header[..16], b"SQLite format 3\0");
    u32::from_be_bytes(
        header[60..64]
            .try_into()
            .expect("SQLite user_version header slice should have four bytes"),
    )
}

fn remove_owned_sqlite_database_and_sidecars(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let owned_path = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::remove_file(&owned_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "fixture-owned SQLite path {} should be removable: {error}",
                owned_path.display()
            ),
        }
    }
}

#[test]
fn db_stats_json_reports_the_ladder() {
    let fixture = TempDatabase::new("stats");
    fixture.initialize_v2();

    let output = fixture.run(&["db", "stats", "--json"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    let value = json["value"]
        .as_object()
        .expect("db stats should wrap its fields in value");
    assert!(value["sampleCount"].is_i64());
    assert!(value.contains_key("oldestCapturedAtMs"));
    assert!(value.contains_key("newestCapturedAtMs"));
    assert!(value["snapshotJsonSampleCount"].is_i64());

    let tiers = value["tiers"]
        .as_array()
        .expect("db stats tiers should be an array");
    assert_eq!(tiers.len(), 4);
    for (tier, expected_name) in tiers.iter().zip(["l1", "l2", "l3", "l4"]) {
        let tier = tier.as_object().expect("tier should be an object");
        assert_eq!(tier["tier"], expected_name);
        assert_eq!(
            key_set(tier),
            BTreeSet::from([
                "bucketCount",
                "enabled",
                "keepDays",
                "newestMs",
                "oldestMs",
                "resolutionMs",
                "tier",
            ])
        );
    }

    let disk = value["disk"].as_object().expect("disk should be an object");
    assert_eq!(
        key_set(disk),
        BTreeSet::from([
            "freeBytes",
            "lastCheckMs",
            "minFreeBytes",
            "pressure",
            "pressureSinceMs",
        ])
    );
    let archive = value["archive"]
        .as_object()
        .expect("archive should be an object");
    assert_eq!(key_set(archive), BTreeSet::from(["cold", "queryable"]));
    assert_eq!(
        key_set(
            archive["queryable"]
                .as_object()
                .expect("queryable archive should be an object")
        ),
        BTreeSet::from(["bucketCount", "enabled", "newestMs", "oldestMs", "path"])
    );
    assert_eq!(
        key_set(
            archive["cold"]
                .as_object()
                .expect("cold archive should be an object")
        ),
        BTreeSet::from([
            "bytes",
            "directory",
            "enabled",
            "exportedUntilMonth",
            "fileCount",
        ])
    );
}

#[tokio::test]
async fn db_stats_json_reports_user_version_2() {
    // Break caught: schema migration succeeds but operators cannot observe the
    // active SQLite schema version through the JSON stats contract.
    let fixture = TempDatabase::new("stats-user-version-v2");
    SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fresh database should initialize")
        .close()
        .await
        .expect("fresh database should close");

    let output = fixture.run(&["db", "stats", "--json"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["value"]["userVersion"], 2);
}

#[tokio::test]
async fn db_stats_reports_otel_presence_only() {
    // Break caught: db stats omits OTel configuration or leaks the configured header value.
    let fixture = TempDatabase::new("stats-otel-presence");
    let store = SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fixture store should connect");
    let mut settings = store.get_settings().await.expect("default settings");
    settings.otel.headers_env_var = "TINYTOP_TEST_HEADER_THAT_IS_NOT_SET".to_string();
    store
        .put_settings(&settings)
        .await
        .expect("OTel settings should persist");
    store.close().await.expect("fixture store should close");

    let output = fixture.run(&["db", "stats", "--json"]);

    assert_success(&output);
    let json = stdout_json(&output);
    let otel = json["value"]["otel"]
        .as_object()
        .expect("db stats should include an OTel object");
    assert_eq!(otel["headersEnvVarSet"], false);
    assert_eq!(otel["headersEnvVar"], "TINYTOP_TEST_HEADER_THAT_IS_NOT_SET");
    assert!(
        !otel.iter().any(|(key, value)| {
            key.contains("Value") || value.as_str() == Some("sekrit-value")
        }),
        "OTel stats must expose presence only: {otel:?}"
    );

    let store = SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fixture store should reconnect");
    let mut settings = store.get_settings().await.expect("stored settings");
    settings.otel.headers_env_var = "TINYTOP_TEST_HEADER_THAT_IS_SET".to_string();
    store
        .put_settings(&settings)
        .await
        .expect("updated OTel settings should persist");
    store.close().await.expect("fixture store should close");

    let output = fixture.run_with_env(
        &["db", "stats", "--json"],
        &[(
            "TINYTOP_TEST_HEADER_THAT_IS_SET",
            "authorization=sekrit-value",
        )],
    );

    assert_success(&output);
    let json = stdout_json(&output);
    let otel = json["value"]["otel"]
        .as_object()
        .expect("db stats should include an OTel object");
    assert_eq!(otel["headersEnvVarSet"], true);
    assert_eq!(otel["headersEnvVar"], "TINYTOP_TEST_HEADER_THAT_IS_SET");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("sekrit-value"), "{stdout}");
    assert!(!stdout.contains("authorization="), "{stdout}");
}

#[tokio::test]
async fn db_stats_shows_disk_pressure_after_a_check() {
    // Break caught: db stats loses the state written by the real disk provider after reopen.
    let fixture = TempDatabase::new("stats-disk-pressure");
    let store = SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fixture store should connect");
    let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
    ladder.disk_check.min_free_bytes = i64::MAX;
    let now = 1_234_567_890;
    check_disk(&store, &SysinfoFreeBytes, &ladder, now)
        .await
        .expect("real disk check should succeed");
    store.close().await.expect("fixture store should close");

    let output = fixture.run(&["db", "stats", "--json"]);

    assert_success(&output);
    eprintln!(
        "db stats disk-pressure acceptance JSON:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let json = stdout_json(&output);
    assert_eq!(json["value"]["disk"]["pressure"], true);
    assert!(
        json["value"]["disk"]["freeBytes"]
            .as_i64()
            .unwrap_or_default()
            > 0
    );
    assert_eq!(json["value"]["disk"]["lastCheckMs"], now);
    assert_eq!(json["value"]["disk"]["pressureSinceMs"], now);
}

#[test]
fn db_stats_closes_store_and_checkpoints_wal() {
    // Break caught: process exit drops the runtime before SQLite checkpoints its last WAL.
    let fixture = TempDatabase::new("stats-checkpoint");
    fixture.initialize_populated_v2();

    let output = fixture.run(&["db", "stats", "--json"]);

    assert_success(&output);
    assert!(!PathBuf::from(format!("{}-wal", fixture.db_path.display())).exists());
}

#[test]
fn db_stats_refuses_a_missing_database() {
    // Break caught: a read-only diagnostic creates the requested DB or any sidecar/directory.
    let fixture = TempDatabase::new("stats-missing");
    let missing_dir = fixture.dir.join("must-not-exist");
    let missing_db = missing_dir.join("h.sqlite");
    let missing_url = format!("sqlite://{}", missing_db.display());

    let output = run_database_command(&missing_url, &["db", "stats", "--json"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    assert_eq!(
        json["reason"],
        format!(
            "database {} does not exist; nothing was created — check the path or start the daemon once",
            missing_db.display()
        )
    );
    assert!(!missing_db.exists());
    assert!(!PathBuf::from(format!("{}-wal", missing_db.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", missing_db.display())).exists());
    assert!(!missing_dir.exists());
}

#[test]
fn db_archive_status_on_fresh_v2_is_read_only() {
    // Break caught: status creates an archive or reports stub cold counters.
    let fixture = TempDatabase::new("archive-status-fresh");
    fixture.initialize_v2();

    let output = fixture.run(&["db", "archive", "status"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["value"]["cold"]["manifest"], serde_json::json!([]));
    assert_eq!(json["value"]["cold"]["fileCount"], 0);
    assert_eq!(json["value"]["cold"]["bytes"], 0);
    assert!(!fixture.dir.join("history-archive.sqlite").exists());
}

#[tokio::test]
async fn db_archive_status_excludes_a_month_that_still_has_main_rows() {
    // Break caught: status promises a month that export-now will skip until its move catches up.
    let fixture = TempDatabase::new("archive-status-main-rows");
    let store = SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fixture store should connect");
    let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
    ladder.l3.enabled = false;
    ladder.l4.keep_days = 30;
    ladder.archive.queryable = true;
    ladder.archive.cold = true;
    ladder.archive.cold_after_months = 1;
    let settings = DashboardSettings {
        retention_ladder: ladder,
        ..DashboardSettings::default()
    };
    store
        .put_settings(&settings)
        .await
        .expect("settings should persist");
    let first = TierBucket {
        bucket_start_ms: JAN_2023_MS,
        first_captured_at_ms: JAN_2023_MS,
        newest_captured_at_ms: JAN_2023_MS + HOUR_MS - 1,
        sample_count: 60,
        cpu: Stat {
            avg: 1.0,
            min: 1.0,
            max: 1.0,
        },
        memory: Stat {
            avg: 1.0,
            min: 1.0,
            max: 1.0,
        },
        swap: Stat {
            avg: 1.0,
            min: 1.0,
            max: 1.0,
        },
        load: Stat {
            avg: 1.0,
            min: 1.0,
            max: 1.0,
        },
        root_used: None,
    };
    let mut second = first.clone();
    second.bucket_start_ms += HOUR_MS;
    second.first_captured_at_ms += HOUR_MS;
    second.newest_captured_at_ms += HOUR_MS;
    store.upsert_tier_bucket(Tier::L4, &first).await.unwrap();
    store.upsert_tier_bucket(Tier::L4, &second).await.unwrap();
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 1).await.unwrap(),
        1
    );
    store.close().await.expect("fixture store should close");

    let output = fixture.run(&["db", "archive", "status"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(
        json["value"]["cold"]["nextExportableMonths"],
        serde_json::json!([])
    );
}

#[tokio::test]
async fn db_archive_status_caps_next_exportable_months_to_one_pass() {
    // Break caught: status promises more months than one export-now pass is allowed to write.
    let fixture = TempDatabase::new("archive-status-month-cap");
    let store = SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fixture store should connect");
    let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
    ladder.l3.enabled = false;
    ladder.l4.keep_days = 30;
    ladder.archive.queryable = true;
    ladder.archive.cold = true;
    ladder.archive.cold_after_months = 1;
    let settings = DashboardSettings {
        retention_ladder: ladder,
        ..DashboardSettings::default()
    };
    store
        .put_settings(&settings)
        .await
        .expect("settings should persist");
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
    for (index, start_ms) in starts.into_iter().enumerate() {
        let stat = Stat {
            avg: index as f64,
            min: index as f64,
            max: index as f64,
        };
        store
            .upsert_tier_bucket(
                Tier::L4,
                &TierBucket {
                    bucket_start_ms: start_ms,
                    first_captured_at_ms: start_ms,
                    newest_captured_at_ms: start_ms + HOUR_MS - 1,
                    sample_count: 60,
                    cpu: stat,
                    memory: stat,
                    swap: stat,
                    load: stat,
                    root_used: None,
                },
            )
            .await
            .expect("fixture L4 bucket should insert");
    }
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 13)
            .await
            .expect("all fixture buckets should move"),
        13
    );
    store.close().await.expect("fixture store should close");

    let output = fixture.run(&["db", "archive", "status"]);

    assert_success(&output);
    let json = stdout_json(&output);
    let next = json["value"]["cold"]["nextExportableMonths"]
        .as_array()
        .expect("status should list ready months");
    assert_eq!(next.len(), 12);
    assert_eq!(next.last(), Some(&serde_json::json!("2023-12")));
}

#[test]
fn db_archive_export_now_refuses_when_cold_is_off() {
    // Break caught: the operator command bypasses its explicit setting gate.
    let fixture = TempDatabase::new("archive-export-refused");
    fixture.initialize_v2();

    let output = fixture.run(&["db", "archive", "export-now"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    let reason = json["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("retentionLadder.archive.cold"));
    assert!(reason.contains("observed false"));
}

#[tokio::test]
async fn db_archive_export_now_writes_seeded_month() {
    // Break caught: the spawned CLI cannot discover/export archive rows seeded through the store.
    let fixture = TempDatabase::new("archive-export-seeded");
    let store = SqliteHistoryStore::connect(&fixture.database_url)
        .await
        .expect("fixture store should connect");
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.l3.enabled = false;
    settings.retention_ladder.l4.keep_days = 30;
    settings.retention_ladder.archive.queryable = true;
    settings.retention_ladder.archive.cold = true;
    settings.retention_ladder.archive.cold_after_months = 1;
    store
        .put_settings(&settings)
        .await
        .expect("settings should persist");
    let stat = Stat {
        avg: 12.5,
        min: 10.0,
        max: 15.0,
    };
    store
        .upsert_tier_bucket(
            Tier::L4,
            &TierBucket {
                bucket_start_ms: JAN_2023_MS,
                first_captured_at_ms: JAN_2023_MS,
                newest_captured_at_ms: JAN_2023_MS + HOUR_MS - 1,
                sample_count: 60,
                cpu: stat,
                memory: stat,
                swap: stat,
                load: stat,
                root_used: Some(stat),
            },
        )
        .await
        .expect("fixture L4 bucket should insert");
    let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
    assert_eq!(
        move_expired_l4(&store, &paths, i64::MAX, 10)
            .await
            .expect("fixture bucket should move"),
        1
    );
    store.close().await.expect("fixture store should close");

    let output = fixture.run(&["db", "archive", "export-now"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["value"]["action"], "export-now");
    assert_eq!(json["value"]["written"].as_array().unwrap().len(), 1);
    assert_eq!(json["value"]["written"][0]["month"], "2023-01");

    let status = fixture.run(&["db", "archive", "status"]);
    assert_success(&status);
    eprintln!(
        "db archive status acceptance JSON:\n{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let status_json = stdout_json(&status);
    assert_eq!(status_json["value"]["cold"]["fileCount"], 1);
    let manifest = status_json["value"]["cold"]["manifest"]
        .as_array()
        .expect("status manifest should be an array");
    assert_eq!(manifest.len(), 1);
    let row = &manifest[0];
    assert_eq!(row["month"], "2023-01");
    assert_eq!(row["file"], "tinytop-1h-2023-01.csv.gz");
    assert_eq!(row["rowCount"], 1);
    let bytes = row["bytes"]
        .as_u64()
        .expect("manifest bytes should be a non-negative integer");
    assert!(bytes > 0);
    assert_eq!(
        bytes,
        fs::metadata(fixture.dir.join("tinytop-1h-2023-01.csv.gz"))
            .expect("cold export should exist")
            .len()
    );
    let sha256 = row["sha256"]
        .as_str()
        .expect("manifest sha256 should be a string");
    assert_eq!(sha256.len(), 64);
    assert!(
        sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert_eq!(
        status_json["value"]["cold"]["exportedUntilMonth"],
        "2023-01"
    );
}

#[test]
fn db_check_never_migrates_a_v0_database() {
    let fixture = TempDatabase::new("check-v0-no-migration");
    fixture.initialize_populated_v2();
    set_sqlite_user_version(&fixture.db_path, 0);

    let output = fixture.run(&["db", "check"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["value"]["result"], "ok");
    assert_eq!(sqlite_user_version(&fixture.db_path), 0);
    assert!(!fixture.pre_image_path.exists());
}

#[test]
fn db_vacuum_never_migrates_a_v0_database() {
    let fixture = TempDatabase::new("vacuum-v0-no-migration");
    fixture.initialize_populated_v2();
    set_sqlite_user_version(&fixture.db_path, 0);

    let output = fixture.run(&["db", "vacuum"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["value"]["action"], "vacuum");
    assert_eq!(sqlite_user_version(&fixture.db_path), 0);
    assert!(!fixture.pre_image_path.exists());
}

#[test]
fn db_stats_refuses_a_v0_database() {
    let fixture = TempDatabase::new("stats-v0-refusal");
    fixture.initialize_populated_v2();
    set_sqlite_user_version(&fixture.db_path, 0);

    let output = fixture.run(&["db", "stats", "--json"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    let reason = json["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("user_version"));
    assert!(reason.contains('0'));
    assert_eq!(sqlite_user_version(&fixture.db_path), 0);
    assert!(!fixture.pre_image_path.exists());
}

#[test]
fn pre_image_status_reports_absence_on_an_existing_v2_database() {
    let fixture = TempDatabase::new("status-absent");
    fixture.initialize_v2();

    let output = fixture.run(&["db", "pre-image", "status"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(
        json["value"]["path"],
        fixture.pre_image_path.display().to_string()
    );
    assert_eq!(json["value"]["exists"], false);
    assert!(json["value"]["bytes"].is_null());
    assert_eq!(json["value"]["databaseExists"], true);
    assert_eq!(json["value"]["userVersion"], 2);
    assert_eq!(json["value"]["integrityCheck"], "ok");
}

#[test]
fn pre_image_remove_refuses_when_database_is_missing() {
    let fixture = TempDatabase::new("remove-missing-database");
    fixture.initialize_v2();
    fs::write(&fixture.pre_image_path, b"only copy").expect("pre-image fixture should be written");
    remove_owned_sqlite_database_and_sidecars(&fixture.db_path);

    let output = fixture.run(&["db", "pre-image", "remove", "--yes"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    let reason = json["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("does not exist"));
    assert!(reason.contains(&fixture.db_path.display().to_string()));
    assert!(fixture.pre_image_path.exists());
    assert!(!fixture.db_path.exists());

    let status = fixture.run(&["db", "pre-image", "status"]);
    assert_success(&status);
    let status_json = stdout_json(&status);
    assert_eq!(status_json["status"], "ok");
    assert_eq!(
        status_json["value"]["path"],
        fixture.pre_image_path.display().to_string()
    );
    assert_eq!(status_json["value"]["databaseExists"], false);
    assert_eq!(status_json["value"]["exists"], true);
    assert!(status_json["value"]["userVersion"].is_null());
    assert!(status_json["value"]["integrityCheck"].is_null());
    assert!(!fixture.db_path.exists());
}

#[cfg(unix)]
#[test]
fn pre_image_status_follows_a_symlinked_database_path() {
    let fixture = TempDatabase::new("symlinked-status");
    let real_dir = fixture.dir.join("real");
    let alias_dir = fixture.dir.join("alias");
    fs::create_dir_all(&real_dir).expect("real database directory should be created");
    let real_db_path = real_dir.join("h.sqlite");
    let real_database_url = format!("sqlite://{}", real_db_path.display());
    let initialize = run_database_command(&real_database_url, &["collect", "--json"]);
    assert_success(&initialize);
    std::os::unix::fs::symlink(&real_dir, &alias_dir)
        .expect("database directory symlink should be created");

    let alias_db_path = alias_dir.join("h.sqlite");
    let alias_database_url = format!("sqlite://{}", alias_db_path.display());
    let canonical_pre_image_path =
        PathBuf::from(format!("{}.pre-v0.sqlite", real_db_path.display()));
    fs::write(&canonical_pre_image_path, b"backup")
        .expect("canonical pre-image fixture should be written");
    let expected_path = canonical_pre_image_path
        .canonicalize()
        .expect("expected pre-image path should canonicalize");

    let status = run_database_command(&alias_database_url, &["db", "pre-image", "status"]);

    assert_success(&status);
    let status_json = stdout_json(&status);
    assert_eq!(status_json["value"]["exists"], true);
    assert_eq!(
        status_json["value"]["path"],
        expected_path.display().to_string()
    );

    let remove = run_database_command(&alias_database_url, &["db", "pre-image", "remove", "--yes"]);
    assert_success(&remove);
    assert!(!canonical_pre_image_path.exists());
    let alias_pre_image_path = PathBuf::from(format!("{}.pre-v0.sqlite", alias_db_path.display()));
    assert!(!alias_pre_image_path.exists());
}

#[test]
fn explicit_sqlite_url_never_touches_the_default_state_directory() {
    let fixture = TempDatabase::new("explicit-sqlite-default-state");
    fixture.initialize_v2();
    let home = fixture.dir.join("home");
    assert!(!home.exists());

    let output = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args(["db", "stats", "--sqlite", &fixture.database_url])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("LOCALAPPDATA", &home)
        .env("XDG_STATE_HOME", &home)
        .env_remove("TINYTOP_HISTORY_DB")
        .output()
        .expect("tinytop-agent database command should run");

    assert_success(&output);
    assert!(!home.exists());
}

#[test]
fn pre_image_status_without_sqlite_does_not_create_default_directories() {
    let fixture = TempDatabase::new("default-status-no-directories");
    let home = fixture.dir.join("home");
    assert!(!home.exists());

    let output = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args(["db", "pre-image", "status"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("LOCALAPPDATA", &home)
        .env("XDG_STATE_HOME", &home)
        .env_remove("TINYTOP_HISTORY_DB")
        .output()
        .expect("tinytop-agent pre-image status command should run");

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["value"]["databaseExists"], false);
    assert!(!home.exists());
}

#[test]
fn pre_image_remove_refuses_without_yes() {
    let fixture = TempDatabase::new("remove-no-yes");
    fixture.initialize_v2();
    File::create(&fixture.pre_image_path).expect("pre-image fixture should be created");

    let output = fixture.run(&["db", "pre-image", "remove"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    assert!(
        json["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("--yes")
    );
    assert!(fixture.pre_image_path.exists());
}

#[test]
fn pre_image_remove_refuses_when_absent() {
    let fixture = TempDatabase::new("remove-absent");
    fixture.initialize_v2();

    let output = fixture.run(&["db", "pre-image", "remove", "--yes"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    assert!(
        json["reason"]
            .as_str()
            .unwrap_or_default()
            .contains(&fixture.pre_image_path.display().to_string())
    );
    assert!(!fixture.pre_image_path.exists());
}

#[test]
fn pre_image_remove_refuses_when_user_version_is_below_1() {
    let fixture = TempDatabase::new("remove-v0");
    fixture.initialize_v2();
    set_sqlite_user_version(&fixture.db_path, 0);
    File::create(&fixture.pre_image_path).expect("pre-image fixture should be created");

    let output = fixture.run(&["db", "pre-image", "remove", "--yes"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["status"], "refused");
    assert!(
        json["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("user_version")
    );
    assert!(fixture.pre_image_path.exists());
}

#[test]
fn pre_image_remove_deletes_after_checks() {
    let fixture = TempDatabase::new("remove-ok");
    fixture.initialize_v2();
    fs::write(&fixture.pre_image_path, b"backup").expect("pre-image fixture should be written");

    let output = fixture.run(&["db", "pre-image", "remove", "--yes"]);

    assert_success(&output);
    let json = stdout_json(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["value"]["action"], "remove");
    assert_eq!(
        json["value"]["path"],
        fixture.pre_image_path.display().to_string()
    );
    assert_eq!(json["value"]["bytes"], 6);
    assert!(!fixture.pre_image_path.exists());
    assert!(fixture.db_path.exists());

    let check = fixture.run(&["db", "check"]);
    assert_success(&check);
    let check_json = stdout_json(&check);
    assert_eq!(check_json["status"], "ok");
    assert_eq!(check_json["value"]["result"], "ok");
}
