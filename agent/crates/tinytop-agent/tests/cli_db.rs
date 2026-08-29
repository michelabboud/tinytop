use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};

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
        Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
            .args(args)
            .args(["--sqlite", &self.database_url])
            .output()
            .expect("tinytop-agent database command should run")
    }

    fn initialize_v1(&self) {
        let output = self.run(&["db", "stats", "--json"]);
        assert_success(&output);
    }
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
    // The agent deliberately uses WAL mode. Its short-lived process can leave
    // page 1 in the fixture WAL, which would override the main-file header
    // below. These are the two exact sidecars owned by this temp fixture.
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

#[test]
fn db_stats_json_reports_the_ladder() {
    let fixture = TempDatabase::new("stats");

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
        BTreeSet::from(["freeBytes", "lastCheckMs", "minFreeBytes", "pressure"])
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

#[test]
fn pre_image_status_reports_absence_on_a_fresh_db() {
    let fixture = TempDatabase::new("status-absent");

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
    assert_eq!(json["value"]["userVersion"], 1);
    assert_eq!(json["value"]["integrityCheck"], "ok");
}

#[test]
fn pre_image_remove_refuses_without_yes() {
    let fixture = TempDatabase::new("remove-no-yes");
    fixture.initialize_v1();
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
    fixture.initialize_v1();

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
    fixture.initialize_v1();
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
    fixture.initialize_v1();
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
