use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use tinytop_store::{HistoryMarkerType, HistoryQuery, SqliteHistoryStore};

struct TempDatabase {
    dir: PathBuf,
    db_path: PathBuf,
    database_url: String,
}

impl TempDatabase {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-cli-config-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("CLI config fixture directory should be created");
        let db_path = dir.join("h.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        Self {
            dir,
            db_path,
            database_url,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        run_config_command(&self.database_url, args)
    }

    fn initialize_v1(&self) {
        let output = self.run(&["collect", "--json"]);
        assert_success(&output);
    }

    fn write_document(&self, name: &str, document: &Value) -> PathBuf {
        let path = self.dir.join(name);
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(document).expect("document should serialize")
            ),
        )
        .expect("config document should be written");
        path
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn run_config_command(database_url: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args(args)
        .args(["--sqlite", database_url])
        .output()
        .expect("tinytop-agent config command should run")
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
        .expect("SQLite user_version fixture update should be durable");
}

#[test]
fn config_export_prints_the_envelope() {
    // Break caught: stdout export omits or misidentifies the versioned settings envelope.
    let fixture = TempDatabase::new("export-envelope");
    fixture.initialize_v1();

    let output = fixture.run(&["config", "export"]);

    assert_success(&output);
    let document = stdout_json(&output);
    assert_eq!(document["tinytopConfigVersion"], 1);
    assert_eq!(document["agentVersion"], env!("CARGO_PKG_VERSION"));
    assert!(document["exportedAtMs"].is_i64());
    assert!(document["settings"].is_object());
}

#[test]
fn config_export_out_writes_atomically_and_refuses_to_overwrite() {
    // Break caught: --out replaces an existing transfer document or strands its temp file.
    let fixture = TempDatabase::new("export-out");
    fixture.initialize_v1();
    let output_path = fixture.dir.join("settings.json");
    let output_path_arg = output_path.to_string_lossy().into_owned();

    let first = fixture.run(&["config", "export", "--out", &output_path_arg]);

    assert_success(&first);
    assert!(output_path.exists());
    let saved: Value = serde_json::from_slice(
        &fs::read(&output_path).expect("exported document should be readable"),
    )
    .expect("exported document should be JSON");
    assert_eq!(saved["tinytopConfigVersion"], 1);
    assert!(!PathBuf::from(format!("{}.tmp", output_path.display())).exists());

    let second = fixture.run(&["config", "export", "--out", &output_path_arg]);

    assert_eq!(second.status.code(), Some(1));
    let refusal = stdout_json(&second);
    assert_eq!(refusal["status"], "refused");
    assert!(refusal["reason"].as_str().is_some_and(|reason| {
        reason.contains(&output_path_arg) && reason.contains("never overwrites")
    }));
    assert!(!PathBuf::from(format!("{}.tmp", output_path.display())).exists());
}

#[test]
fn config_import_dry_run_prints_the_plan_and_exits_1_on_errors() {
    // Break caught: an invalid dry-run exits successfully or hides the structured validation plan.
    let fixture = TempDatabase::new("import-dry-run-invalid");
    fixture.initialize_v1();
    let document_path = fixture.write_document(
        "newer.json",
        &json!({"tinytopConfigVersion": 2, "settings": {}}),
    );
    let document_path_arg = document_path.to_string_lossy().into_owned();

    let output = fixture.run(&["config", "import", &document_path_arg, "--dry-run"]);

    assert_eq!(output.status.code(), Some(1));
    let plan = stdout_json(&output);
    assert_eq!(plan["valid"], false);
    assert!(plan["errors"].as_array().is_some_and(|errors| {
        errors.iter().any(|error| {
            error
                .as_str()
                .is_some_and(|message| message.contains("maximum supported 1"))
        })
    }));
    assert!(plan["changedKeys"].is_array());
    assert!(plan["wouldDelete"].is_object());
}

#[tokio::test]
async fn config_import_refuses_an_invalid_document_with_one_refused_object() {
    // Break caught: a real invalid import prints multiple objects, mutates settings, or records a marker.
    let fixture = TempDatabase::new("import-invalid-real");
    fixture.initialize_v1();
    let before_output = fixture.run(&["config", "export"]);
    assert_success(&before_output);
    let mut before = stdout_json(&before_output);
    let mut invalid = before.clone();
    invalid["tinytopConfigVersion"] = json!(2);
    let document_path = fixture.write_document("newer.json", &invalid);
    let document_path_arg = document_path.to_string_lossy().into_owned();

    let output = fixture.run(&["config", "import", &document_path_arg]);

    assert_eq!(output.status.code(), Some(1));
    let refusal = stdout_json(&output);
    assert_eq!(refusal["status"], "refused");
    assert!(
        refusal["reason"]
            .as_str()
            .is_some_and(|reason| reason.starts_with("settings document invalid:"))
    );
    assert_eq!(refusal["details"]["valid"], false);
    assert!(
        refusal["details"]["errors"][0]
            .as_str()
            .is_some_and(|error| error.contains("maximum supported 1"))
    );

    let after_output = fixture.run(&["config", "export"]);
    assert_success(&after_output);
    let mut after = stdout_json(&after_output);
    before["exportedAtMs"] = json!(0);
    after["exportedAtMs"] = json!(0);
    assert_eq!(after, before);

    let store = SqliteHistoryStore::connect_for_inspection(&fixture.database_url)
        .await
        .expect("fixture store should reopen for marker inspection");
    let markers = store
        .read_history_markers(
            HistoryQuery {
                since_ms: None,
                until_ms: None,
                limit: Some(100),
            },
            60_000,
        )
        .await
        .expect("history markers should read");
    store.close().await.expect("fixture store should close");
    assert!(
        markers.is_empty(),
        "invalid import must not record a marker"
    );
}

#[tokio::test]
async fn config_import_round_trip_applies_and_records_the_marker() {
    // Break caught: CLI import reports success without persisting the edit and its import marker.
    let fixture = TempDatabase::new("import-round-trip");
    fixture.initialize_v1();

    let before_output = fixture.run(&["config", "export"]);
    assert_success(&before_output);
    let mut edited = stdout_json(&before_output);
    edited["settings"]["retentionLadder"]["l2"]["keepDays"] = json!(31);
    let document_path = fixture.write_document("edited.json", &edited);
    let document_path_arg = document_path.to_string_lossy().into_owned();

    let import_output = fixture.run(&["config", "import", &document_path_arg]);
    assert_success(&import_output);
    let import_result = stdout_json(&import_output);
    assert_eq!(import_result["status"], "ok");
    assert_eq!(import_result["action"], "import");
    assert_eq!(
        import_result["maintenance"],
        "deferred to the daemon's next tick"
    );

    let after_output = fixture.run(&["config", "export"]);
    assert_success(&after_output);
    let after = stdout_json(&after_output);
    assert_eq!(after["settings"]["retentionLadder"]["l2"]["keepDays"], 31);
    assert_eq!(after["settings"]["rollupRetentionDays"], 31);

    let store = SqliteHistoryStore::connect_for_inspection(&fixture.database_url)
        .await
        .expect("fixture store should reopen for marker inspection");
    let markers = store
        .read_history_markers(
            HistoryQuery {
                since_ms: None,
                until_ms: None,
                limit: Some(100),
            },
            60_000,
        )
        .await
        .expect("history markers should read");
    store.close().await.expect("fixture store should close");
    assert!(markers.iter().any(|marker| {
        marker.marker_type == HistoryMarkerType::SettingsChange
            && marker.label == "Settings imported"
            && marker.details["source"] == "import"
            && marker.details["changed"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|key| key == "retentionLadder"))
    }));

    let mut transcript_before = stdout_json(&before_output);
    transcript_before["exportedAtMs"] = json!(0);
    let mut transcript_after = after;
    transcript_after["exportedAtMs"] = json!(0);
    eprintln!(
        "config round-trip transcript (exportedAtMs normalised to 0):\n$ tinytop-agent config export\n{}\nEDIT settings.retentionLadder.l2.keepDays = 31\n$ tinytop-agent config import edited.json\n{}\n$ tinytop-agent config export\n{}",
        serde_json::to_string_pretty(&transcript_before).unwrap(),
        serde_json::to_string_pretty(&import_result).unwrap(),
        serde_json::to_string_pretty(&transcript_after).unwrap(),
    );
}

#[test]
fn config_import_refuses_a_missing_database_and_creates_nothing() {
    // Break caught: import creates or migrates the database it was asked to inspect.
    let fixture = TempDatabase::new("import-missing-database");
    let document_path = fixture.write_document(
        "settings.json",
        &json!({"tinytopConfigVersion": 1, "settings": {}}),
    );
    let document_path_arg = document_path.to_string_lossy().into_owned();
    let missing_dir = fixture.dir.join("must-not-exist");
    let missing_db = missing_dir.join("h.sqlite");
    let missing_url = format!("sqlite://{}", missing_db.display());

    let output = run_config_command(
        &missing_url,
        &["config", "import", &document_path_arg, "--dry-run"],
    );

    assert_eq!(output.status.code(), Some(1));
    let refusal = stdout_json(&output);
    assert_eq!(refusal["status"], "refused");
    assert!(refusal["reason"].as_str().is_some_and(|reason| {
        reason.contains(&missing_db.display().to_string()) && reason.contains("nothing was created")
    }));
    assert!(!missing_db.exists());
    assert!(!PathBuf::from(format!("{}-wal", missing_db.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", missing_db.display())).exists());
    assert!(!missing_dir.exists());
}

#[test]
fn config_import_refuses_a_v0_database() {
    // Break caught: config import silently migrates a v0 database instead of refusing it.
    let fixture = TempDatabase::new("import-v0");
    fixture.initialize_v1();
    set_sqlite_user_version(&fixture.db_path, 0);
    let document_path = fixture.write_document(
        "settings.json",
        &json!({"tinytopConfigVersion": 1, "settings": {}}),
    );
    let document_path_arg = document_path.to_string_lossy().into_owned();

    let output = fixture.run(&["config", "import", &document_path_arg, "--dry-run"]);

    assert_eq!(output.status.code(), Some(1));
    let refusal = stdout_json(&output);
    assert_eq!(refusal["status"], "refused");
    assert!(refusal["reason"].as_str().is_some_and(|reason| {
        reason.contains("user_version check observed 0")
            && reason.contains("schema v1 migration has not run")
    }));
}
