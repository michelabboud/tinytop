use serde_json::{Value, json};
use tinytop_store::{
    DashboardSettings, SqliteHistoryStore, StoreError, apply_disk_measurement,
    ladder::Tier,
    retention_ladder::{DiskPressureState, RetentionLadder},
};
use tinytop_types::{
    CpuSnapshot, CpuTimes, FilesystemSnapshot, IdentitySnapshot, LoadSnapshot, MemorySnapshot,
    PressureGroup, PressureSnapshot, ProcessSnapshot, RuntimeConfidence, RuntimeDetection,
    RuntimeKind, SwapSnapshot, SystemSnapshot,
};

struct InvalidCase {
    name: &'static str,
    ladder: Value,
    expected: &'static str,
}

fn valid_ladder() -> Value {
    json!({
        "l1": { "keepDays": 3 },
        "l2": { "keepDays": 30 },
        "l3": { "enabled": true, "keepDays": 90 },
        "l4": { "enabled": true, "keepDays": 730 },
        "snapshotJsonKeepMinutes": 60,
        "detailIntervalSec": 60,
        "archive": {
            "queryable": false,
            "cold": false,
            "coldAfterMonths": 12,
            "directory": ""
        },
        "diskCheck": {
            "intervalMinutes": 60,
            "minFreeBytes": 5_368_709_120_i64
        }
    })
}

fn with_field(path: &[&str], value: Value) -> Value {
    let mut ladder = valid_ladder();
    let mut current = &mut ladder;
    for key in &path[..path.len() - 1] {
        current = current
            .get_mut(*key)
            .unwrap_or_else(|| panic!("fixture path segment {key} should exist"));
    }
    current[path[path.len() - 1]] = value;
    ladder
}

fn settings_document_without_ladder(settings: &DashboardSettings) -> Value {
    let mut document =
        serde_json::to_value(settings).expect("dashboard settings should serialize to JSON");
    document
        .as_object_mut()
        .expect("dashboard settings should serialize to an object")
        .remove("retentionLadder");
    document
}

fn invalid_cases() -> Vec<InvalidCase> {
    let mut l4_below_l2 = valid_ladder();
    l4_below_l2["l3"]["enabled"] = json!(false);
    l4_below_l2["l4"]["keepDays"] = json!(29);

    vec![
        InvalidCase {
            name: "l1 below minimum",
            ladder: with_field(&["l1", "keepDays"], json!(2)),
            expected: "retentionLadder.l1.keepDays must be between 3 and 3650; observed 2",
        },
        InvalidCase {
            name: "l1 above maximum",
            ladder: with_field(&["l1", "keepDays"], json!(3651)),
            expected: "retentionLadder.l1.keepDays must be between 3 and 3650; observed 3651",
        },
        InvalidCase {
            name: "l2 below minimum",
            ladder: with_field(&["l2", "keepDays"], json!(6)),
            expected: "retentionLadder.l2.keepDays must be between 7 and 3650; observed 6",
        },
        InvalidCase {
            name: "l2 above maximum",
            ladder: with_field(&["l2", "keepDays"], json!(3651)),
            expected: "retentionLadder.l2.keepDays must be between 7 and 3650; observed 3651",
        },
        InvalidCase {
            name: "l3 above maximum",
            ladder: {
                let mut ladder = with_field(&["l3", "keepDays"], json!(3651));
                ladder["l3"]["enabled"] = json!(false);
                ladder
            },
            expected: "retentionLadder.l3.keepDays must be between 0 and 3650; observed 3651",
        },
        InvalidCase {
            name: "l4 below zero",
            ladder: {
                let mut ladder = with_field(&["l4", "keepDays"], json!(-1));
                ladder["l4"]["enabled"] = json!(false);
                ladder
            },
            expected: "retentionLadder.l4.keepDays must be between 0 and 36500; observed -1",
        },
        InvalidCase {
            name: "l4 above maximum",
            ladder: {
                let mut ladder = with_field(&["l4", "keepDays"], json!(36_501));
                ladder["l4"]["enabled"] = json!(false);
                ladder
            },
            expected: "retentionLadder.l4.keepDays must be between 0 and 36500; observed 36501",
        },
        InvalidCase {
            name: "snapshot JSON below minimum",
            ladder: with_field(&["snapshotJsonKeepMinutes"], json!(59)),
            expected: "retentionLadder.snapshotJsonKeepMinutes must be between 60 and 1440; observed 59",
        },
        InvalidCase {
            name: "snapshot JSON above maximum",
            ladder: with_field(&["snapshotJsonKeepMinutes"], json!(1441)),
            expected: "retentionLadder.snapshotJsonKeepMinutes must be between 60 and 1440; observed 1441",
        },
        InvalidCase {
            name: "detail interval below minimum",
            ladder: with_field(&["detailIntervalSec"], json!(14)),
            expected: "retentionLadder.detailIntervalSec must be between 15 and 3600; observed 14",
        },
        InvalidCase {
            name: "detail interval above maximum",
            ladder: with_field(&["detailIntervalSec"], json!(3601)),
            expected: "retentionLadder.detailIntervalSec must be between 15 and 3600; observed 3601",
        },
        InvalidCase {
            name: "cold-after below minimum",
            ladder: with_field(&["archive", "coldAfterMonths"], json!(0)),
            expected: "retentionLadder.archive.coldAfterMonths must be between 1 and 120; observed 0",
        },
        InvalidCase {
            name: "cold-after above maximum",
            ladder: with_field(&["archive", "coldAfterMonths"], json!(121)),
            expected: "retentionLadder.archive.coldAfterMonths must be between 1 and 120; observed 121",
        },
        InvalidCase {
            name: "disk interval below minimum",
            ladder: with_field(&["diskCheck", "intervalMinutes"], json!(4)),
            expected: "retentionLadder.diskCheck.intervalMinutes must be between 5 and 1440; observed 4",
        },
        InvalidCase {
            name: "disk interval above maximum",
            ladder: with_field(&["diskCheck", "intervalMinutes"], json!(1441)),
            expected: "retentionLadder.diskCheck.intervalMinutes must be between 5 and 1440; observed 1441",
        },
        InvalidCase {
            name: "minimum free bytes below minimum",
            ladder: with_field(&["diskCheck", "minFreeBytes"], json!(268_435_455)),
            expected: "retentionLadder.diskCheck.minFreeBytes must be at least 268435456; observed 268435455",
        },
        InvalidCase {
            name: "l3 below l2",
            ladder: with_field(&["l3", "keepDays"], json!(29)),
            expected: "retentionLadder.l3.keepDays must be greater than or equal to retentionLadder.l2.keepDays (30) when retentionLadder.l3.enabled is true; observed 29",
        },
        InvalidCase {
            name: "l4 below enabled l3",
            ladder: with_field(&["l4", "keepDays"], json!(89)),
            expected: "retentionLadder.l4.keepDays must be 0 (forever) or greater than or equal to retentionLadder.l3.keepDays (90) when retentionLadder.l4.enabled is true; observed 89",
        },
        InvalidCase {
            name: "l4 below l2 when l3 is disabled",
            ladder: l4_below_l2,
            expected: "retentionLadder.l4.keepDays must be 0 (forever) or greater than or equal to retentionLadder.l2.keepDays (30) when retentionLadder.l4.enabled is true; observed 29",
        },
        InvalidCase {
            name: "cold archive without queryable archive",
            ladder: with_field(&["archive", "cold"], json!(true)),
            expected: "retentionLadder.archive.cold requires retentionLadder.archive.queryable=true; observed cold=true, queryable=false",
        },
        InvalidCase {
            name: "relative archive directory",
            ladder: with_field(&["archive", "directory"], json!("relative/archive")),
            expected: "retentionLadder.archive.directory must be empty or an absolute path; observed \"relative/archive\"",
        },
    ]
}

#[test]
fn retention_ladder_validation_rejects_every_invalid_shape_with_exact_error() {
    let mut failures = Vec::new();

    for case in invalid_cases() {
        let mut document = serde_json::to_value(DashboardSettings::default())
            .expect("default settings should serialize");
        document["retentionLadder"] = case.ladder;
        let settings: DashboardSettings =
            serde_json::from_value(document).expect("candidate settings should deserialize");

        match settings.validate() {
            Err(StoreError::Validation(message)) if message == case.expected => {}
            result => failures.push(format!(
                "{}: expected {:?}, observed {result:?}",
                case.name, case.expected
            )),
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn retention_ladder_accepts_boundaries_and_forever() {
    let mut ladder = RetentionLadder::default();
    ladder.l1.keep_days = 3;
    ladder.l2.keep_days = 7;
    ladder.l3.keep_days = 7;
    ladder.l4.keep_days = 0;
    ladder.snapshot_json_keep_minutes = 60;
    ladder.detail_interval_sec = 15;
    ladder.archive.cold_after_months = 1;
    ladder.disk_check.interval_minutes = 5;
    ladder.disk_check.min_free_bytes = 256 * 1024 * 1024;

    ladder
        .validate(None, None)
        .expect("documented lower bounds and L4 forever should be valid");
}

#[test]
fn validate_emits_the_spec_disk_pressure_message() {
    let previous = RetentionLadder::default();
    let mut growing = previous.clone();
    growing.l2.keep_days += 1;
    let active_pressure = DiskPressureState {
        active: true,
        since_ms: None,
        free_bytes: 1_000,
        min_free_bytes: 5_000,
    };

    let error = growing
        .validate(Some(&active_pressure), Some(&previous))
        .expect_err("growth should be refused while disk pressure is active");
    assert_eq!(
        error.to_string(),
        "disk pressure active: free 1000 < minFreeBytes 5000; shrink first or free disk"
    );

    let inactive_pressure = DiskPressureState {
        active: false,
        since_ms: None,
        ..active_pressure
    };
    growing
        .validate(Some(&inactive_pressure), Some(&previous))
        .expect("inactive disk pressure should allow growth");
}

#[test]
fn disk_pressure_rule_table() {
    struct PressureCase {
        name: &'static str,
        configure: fn(&mut RetentionLadder, &mut RetentionLadder),
        refused: bool,
    }

    let cases = [
        PressureCase {
            name: "l1 +1",
            configure: |_, candidate| candidate.l1.keep_days += 1,
            refused: true,
        },
        PressureCase {
            name: "l2 +1",
            configure: |_, candidate| candidate.l2.keep_days += 1,
            refused: true,
        },
        PressureCase {
            name: "l3 enabled keepDays +1",
            configure: |_, candidate| candidate.l3.keep_days += 1,
            refused: true,
        },
        PressureCase {
            name: "l3 enable",
            configure: |previous, _| previous.l3.enabled = false,
            refused: true,
        },
        PressureCase {
            name: "l3 disabled keepDays +1",
            configure: |previous, candidate| {
                previous.l3.enabled = false;
                candidate.l3.enabled = false;
                candidate.l3.keep_days += 1;
            },
            refused: false,
        },
        PressureCase {
            name: "l4 enable",
            configure: |previous, _| previous.l4.enabled = false,
            refused: true,
        },
        PressureCase {
            name: "l4 finite to forever",
            configure: |_, candidate| candidate.l4.keep_days = 0,
            refused: true,
        },
        PressureCase {
            name: "l4 forever to finite",
            configure: |previous, _| previous.l4.keep_days = 0,
            refused: false,
        },
        PressureCase {
            name: "l4 finite +1",
            configure: |_, candidate| candidate.l4.keep_days += 1,
            refused: true,
        },
        PressureCase {
            name: "snapshotJsonKeepMinutes +1",
            configure: |_, candidate| candidate.snapshot_json_keep_minutes += 1,
            refused: true,
        },
        PressureCase {
            name: "archive.queryable enable",
            configure: |_, candidate| candidate.archive.queryable = true,
            refused: true,
        },
        PressureCase {
            name: "archive.cold enable",
            configure: |previous, candidate| {
                previous.archive.queryable = true;
                candidate.archive.queryable = true;
                candidate.archive.cold = true;
            },
            refused: true,
        },
        PressureCase {
            name: "detailIntervalSec change",
            configure: |_, candidate| candidate.detail_interval_sec += 1,
            refused: false,
        },
        PressureCase {
            name: "diskCheck.* change",
            configure: |_, candidate| {
                candidate.disk_check.interval_minutes += 1;
                candidate.disk_check.min_free_bytes += 1;
            },
            refused: false,
        },
        PressureCase {
            name: "l1 shrink",
            configure: |previous, _| previous.l1.keep_days += 1,
            refused: false,
        },
        PressureCase {
            name: "l2 shrink",
            configure: |previous, _| previous.l2.keep_days += 1,
            refused: false,
        },
        PressureCase {
            name: "l3 shrink",
            configure: |previous, _| previous.l3.keep_days += 1,
            refused: false,
        },
        PressureCase {
            name: "l4 shrink",
            configure: |previous, _| previous.l4.keep_days += 1,
            refused: false,
        },
    ];
    let active_pressure = DiskPressureState {
        active: true,
        since_ms: None,
        free_bytes: 1_000,
        min_free_bytes: 5_000,
    };
    let inactive_pressure = DiskPressureState {
        active: false,
        since_ms: None,
        free_bytes: 1_000,
        min_free_bytes: 5_000,
    };
    let expected_message =
        "disk pressure active: free 1000 < minFreeBytes 5000; shrink first or free disk";
    let mut failures = Vec::new();

    for case in cases {
        let mut previous = RetentionLadder::default();
        let mut candidate = previous.clone();
        (case.configure)(&mut previous, &mut candidate);

        match (
            case.refused,
            candidate.validate(Some(&active_pressure), Some(&previous)),
        ) {
            (true, Err(StoreError::Validation(message))) if message == expected_message => {}
            (false, Ok(())) => {}
            (_, result) => failures.push(format!(
                "{} with active pressure: expected refused={}, observed {result:?}",
                case.name, case.refused
            )),
        }

        if let Err(error) = candidate.validate(Some(&inactive_pressure), Some(&previous)) {
            failures.push(format!(
                "{} with inactive pressure: expected Ok, observed {error}",
                case.name
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn legacy_aliases_derive_ladder_with_minimums() {
    let normal = RetentionLadder::from_legacy(72, 30);
    assert_eq!(normal.l1.keep_days, 3);
    assert_eq!(normal.l2.keep_days, 30);

    let clamped = RetentionLadder::from_legacy(24, 2);
    assert_eq!(clamped.l1.keep_days, 3);
    assert_eq!(clamped.l2.keep_days, 7);
}

#[test]
fn from_document_without_ladder_merges_legacy_aliases_onto_the_persisted_ladder() {
    let mut persisted_ladder = RetentionLadder::default();
    persisted_ladder.l3.enabled = false;
    persisted_ladder.l4.keep_days = 0;
    persisted_ladder.detail_interval_sec = 30;
    persisted_ladder.archive.queryable = true;
    let persisted = DashboardSettings {
        retention_ladder: persisted_ladder.clone(),
        ..DashboardSettings::default()
    };
    let mut document = settings_document_without_ladder(&DashboardSettings::default());
    document["retentionHours"] = json!(96);
    document["rollupRetentionDays"] = json!(14);

    let settings = DashboardSettings::from_document(document, Some(&persisted))
        .expect("legacy settings document should decode");

    assert_eq!(settings.retention_ladder.l1.keep_days, 4);
    assert_eq!(settings.retention_ladder.l2.keep_days, 14);
    assert_eq!(settings.retention_ladder.l3, persisted_ladder.l3);
    assert_eq!(settings.retention_ladder.l4, persisted_ladder.l4);
    assert_eq!(settings.retention_ladder.detail_interval_sec, 30);
    assert!(settings.retention_ladder.archive.queryable);
}

#[test]
fn from_document_with_persisted_ladder_never_resets_customised_toggles() {
    let mut persisted_ladder = RetentionLadder::default();
    persisted_ladder.l3.enabled = false;
    persisted_ladder.l4.keep_days = 0;
    persisted_ladder.archive.directory = "/x".to_string();
    let persisted = DashboardSettings {
        retention_ladder: persisted_ladder.clone(),
        ..DashboardSettings::default()
    };
    let mut document = settings_document_without_ladder(&DashboardSettings::default());
    document["retentionHours"] = json!(96);

    let settings = DashboardSettings::from_document(document, Some(&persisted))
        .expect("legacy settings document should decode");

    assert_eq!(settings.retention_ladder.l1.keep_days, 4);
    assert!(!settings.retention_ladder.l3.enabled);
    assert_eq!(settings.retention_ladder.l4.keep_days, 0);
    assert_eq!(settings.retention_ladder.archive.directory, "/x");
}

#[test]
fn partial_nested_ladder_objects_keep_contextual_defaults() {
    let defaults = RetentionLadder::default();
    let cases = [
        (json!({}), defaults.clone()),
        (json!({ "l1": {} }), defaults.clone()),
        (json!({ "l2": {} }), defaults.clone()),
        (json!({ "l3": {} }), defaults.clone()),
        (
            json!({ "l3": { "enabled": false } }),
            RetentionLadder {
                l3: tinytop_store::retention_ladder::ToggledTierKeep {
                    enabled: false,
                    keep_days: 90,
                },
                ..defaults.clone()
            },
        ),
        (
            json!({ "l3": { "keepDays": 100 } }),
            RetentionLadder {
                l3: tinytop_store::retention_ladder::ToggledTierKeep {
                    enabled: true,
                    keep_days: 100,
                },
                ..defaults.clone()
            },
        ),
        (json!({ "l4": {} }), defaults.clone()),
        (json!({ "l4": { "enabled": true } }), defaults.clone()),
        (
            json!({ "l4": { "keepDays": 800 } }),
            RetentionLadder {
                l4: tinytop_store::retention_ladder::ToggledTierKeep {
                    enabled: true,
                    keep_days: 800,
                },
                ..defaults.clone()
            },
        ),
        (json!({ "archive": {} }), defaults.clone()),
        (json!({ "diskCheck": {} }), defaults.clone()),
    ];

    for (document, expected) in cases {
        let observed: RetentionLadder =
            serde_json::from_value(document.clone()).expect("partial ladder should deserialize");
        assert_eq!(observed, expected, "document {document}");
    }
}

#[test]
fn explicit_null_in_partial_ladder_is_rejected() {
    for document in [
        json!({ "l1": { "keepDays": null } }),
        json!({ "l3": { "enabled": null } }),
        json!({ "l4": { "keepDays": null } }),
    ] {
        assert!(
            serde_json::from_value::<RetentionLadder>(document.clone()).is_err(),
            "explicit null must not receive a default: {document}"
        );
    }
}

#[test]
fn retention_ladder_maps_every_maintenance_setting() {
    let mut ladder = RetentionLadder::default();
    ladder.l1.keep_days = 4;
    ladder.l2.keep_days = 31;
    ladder.l3.enabled = false;
    ladder.l4.keep_days = 0;
    ladder.snapshot_json_keep_minutes = 75;
    ladder.detail_interval_sec = 45;

    let config = ladder.to_ladder_config(2_000);
    assert_eq!(config.l1_keep_ms, 4 * 86_400_000);
    assert_eq!(config.l2_keep_ms, 31 * 86_400_000);
    assert_eq!(config.l3, None);
    assert_eq!(config.l4, Some(0));
    assert_eq!(config.snapshot_json_keep_ms, 75 * 60_000);
    assert_eq!(config.detail_interval_ms, 45_000);
    assert_eq!(config.poll_interval_ms, 2_000);
}

#[tokio::test]
async fn settings_save_round_trip_writes_legacy_mirrors() {
    let (dir, database_url) = temp_database("round-trip");
    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should connect");
    let default_saved = store
        .put_settings(&DashboardSettings::default())
        .await
        .expect("default settings should save");
    assert_eq!(default_saved.retention_hours, 72);
    assert_eq!(default_saved.rollup_retention_days, 30);
    drop(store);

    let pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("raw database should open");
    let value_json: String =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE setting_key = 'dashboard'")
            .fetch_one(&pool)
            .await
            .expect("stored default settings JSON");
    let document: Value =
        serde_json::from_str(&value_json).expect("stored default settings should be JSON");
    assert_eq!(document["retentionHours"], json!(72));
    assert_eq!(document["rollupRetentionDays"], json!(30));
    pool.close().await;

    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should reopen for customized settings");
    let mut candidate = DashboardSettings::default();
    candidate.retention_ladder.l1.keep_days = 4;
    candidate.retention_ladder.l2.keep_days = 31;
    candidate.retention_hours = 999;
    candidate.rollup_retention_days = 999;

    let saved = store
        .put_settings(&candidate)
        .await
        .expect("settings should save");
    assert_eq!(saved.retention_hours, 96);
    assert_eq!(saved.rollup_retention_days, 31);
    drop(store);

    let pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("raw database should open");
    let value_json: String =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE setting_key = 'dashboard'")
            .fetch_one(&pool)
            .await
            .expect("stored settings JSON");
    let document: Value =
        serde_json::from_str(&value_json).expect("stored settings should be JSON");
    assert_eq!(document["retentionHours"], json!(96));
    assert_eq!(document["rollupRetentionDays"], json!(31));
    assert!(document.get("retentionLadder").is_some());
    pool.close().await;

    let reopened = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should reopen");
    let loaded = reopened.get_settings().await.expect("settings should load");
    assert_eq!(loaded, saved);
    drop(reopened);
    std::fs::remove_dir_all(dir).expect("owned temp fixture should be removable");
}

#[tokio::test]
async fn consecutive_legacy_edits_work_and_explicit_ladder_reset_wins() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    let previous = store
        .get_settings()
        .await
        .expect("default settings should load");
    let mut first_document = settings_document_without_ladder(&previous);
    first_document["retentionHours"] = json!(96);
    let first_legacy_edit =
        DashboardSettings::from_document(first_document, Some(&previous))
            .expect("first legacy document should decode");
    let first = store
        .put_settings(&first_legacy_edit)
        .await
        .expect("first legacy edit should save");
    assert_eq!(first.retention_ladder.l1.keep_days, 4);

    let mut second_document = settings_document_without_ladder(&first);
    second_document["retentionHours"] = json!(120);
    let second_legacy_edit =
        DashboardSettings::from_document(second_document, Some(&first))
            .expect("second legacy document should decode");
    let second = store
        .put_settings(&second_legacy_edit)
        .await
        .expect("second legacy edit should save");
    assert_eq!(second.retention_ladder.l1.keep_days, 5);
    assert_eq!(second.retention_hours, 120);

    let mut explicit_ladder_reset = second;
    explicit_ladder_reset.retention_ladder = RetentionLadder::default();
    let reset = store
        .put_settings(&explicit_ladder_reset)
        .await
        .expect("explicit ladder reset should win over stale aliases");
    assert_eq!(reset.retention_ladder, RetentionLadder::default());
    assert_eq!(reset.retention_hours, 72);
}

#[tokio::test]
async fn legacy_alias_edit_preserves_non_legacy_ladder_settings() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    let mut baseline = DashboardSettings::default();
    baseline.retention_ladder.l3.enabled = false;
    baseline.retention_ladder.l4.keep_days = 0;
    baseline.retention_ladder.snapshot_json_keep_minutes = 120;
    baseline.retention_ladder.detail_interval_sec = 30;
    baseline.retention_ladder.archive.queryable = true;
    baseline.retention_ladder.disk_check.interval_minutes = 15;
    let baseline = store
        .put_settings(&baseline)
        .await
        .expect("non-default baseline should save");

    let mut legacy_document = settings_document_without_ladder(&baseline);
    legacy_document["retentionHours"] = json!(96);
    let legacy_edit =
        DashboardSettings::from_document(legacy_document, Some(&baseline))
            .expect("legacy document should decode");
    let saved = store
        .put_settings(&legacy_edit)
        .await
        .expect("legacy edit should save");

    assert_eq!(saved.retention_ladder.l1.keep_days, 4);
    assert_eq!(saved.retention_ladder.l3, baseline.retention_ladder.l3);
    assert_eq!(saved.retention_ladder.l4, baseline.retention_ladder.l4);
    assert_eq!(
        saved.retention_ladder.snapshot_json_keep_minutes,
        baseline.retention_ladder.snapshot_json_keep_minutes
    );
    assert_eq!(
        saved.retention_ladder.detail_interval_sec,
        baseline.retention_ladder.detail_interval_sec
    );
    assert_eq!(
        saved.retention_ladder.archive,
        baseline.retention_ladder.archive
    );
    assert_eq!(
        saved.retention_ladder.disk_check,
        baseline.retention_ladder.disk_check
    );
}

#[tokio::test]
async fn stored_legacy_document_loads_derived_ladder_without_rewriting() {
    let (dir, database_url) = temp_database("legacy-load");
    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should initialize schema");
    drop(store);

    let mut document = serde_json::to_value(DashboardSettings::default())
        .expect("default settings should serialize");
    document
        .as_object_mut()
        .expect("settings should be an object")
        .remove("retentionLadder");
    document["retentionHours"] = json!(96);
    document["rollupRetentionDays"] = json!(14);
    let legacy_json = serde_json::to_string(&document).expect("legacy document should serialize");

    let pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("raw database should open");
    sqlx::query(
        "INSERT INTO app_settings (setting_key, value_json, updated_at_ms) VALUES ('dashboard', ?, 1)",
    )
    .bind(&legacy_json)
    .execute(&pool)
    .await
    .expect("legacy settings should insert");
    pool.close().await;

    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should reopen");
    let loaded = store
        .get_settings()
        .await
        .expect("legacy settings should load");
    assert_eq!(loaded.retention_ladder.l1.keep_days, 4);
    assert_eq!(loaded.retention_ladder.l2.keep_days, 14);
    drop(store);

    let pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("raw database should reopen");
    let after_read: String =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE setting_key = 'dashboard'")
            .fetch_one(&pool)
            .await
            .expect("stored settings JSON");
    let after_read: Value =
        serde_json::from_str(&after_read).expect("stored settings should be JSON");
    assert!(after_read.get("retentionLadder").is_none());
    pool.close().await;
    std::fs::remove_dir_all(dir).expect("owned temp fixture should be removable");
}

#[tokio::test]
async fn stored_legacy_long_rollup_horizon_raises_enabled_ancestor_defaults() {
    let (dir, database_url) = temp_database("legacy-long-rollup");
    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should initialize schema");
    drop(store);

    let mut document = serde_json::to_value(DashboardSettings::default())
        .expect("default settings should serialize");
    document
        .as_object_mut()
        .expect("settings should be an object")
        .remove("retentionLadder");
    document["rollupRetentionDays"] = json!(366);
    let legacy_json = serde_json::to_string(&document).expect("legacy document should serialize");

    let pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("raw database should open");
    sqlx::query(
        "INSERT INTO app_settings (setting_key, value_json, updated_at_ms) VALUES ('dashboard', ?, 1)",
    )
    .bind(&legacy_json)
    .execute(&pool)
    .await
    .expect("legacy settings should insert");
    pool.close().await;

    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should reopen");
    let loaded = store
        .get_settings()
        .await
        .expect("formerly valid legacy horizon should still load");
    assert_eq!(loaded.retention_ladder.l2.keep_days, 366);
    assert_eq!(loaded.retention_ladder.l3.keep_days, 366);
    assert_eq!(loaded.retention_ladder.l4.keep_days, 730);
    drop(store);

    let pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("raw database should reopen");
    let after_read: String =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE setting_key = 'dashboard'")
            .fetch_one(&pool)
            .await
            .expect("stored settings JSON");
    let after_read: Value =
        serde_json::from_str(&after_read).expect("stored settings should be JSON");
    assert!(after_read.get("retentionLadder").is_none());
    pool.close().await;
    std::fs::remove_dir_all(dir).expect("owned temp fixture should be removable");
}

#[tokio::test]
async fn disk_pressure_refuses_growth_but_allows_shrink() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    let previous = store
        .put_settings(&DashboardSettings::default())
        .await
        .expect("baseline settings should save");
    set_disk_pressure(&store).await;

    let mut extending = previous.clone();
    extending.retention_ladder.l2.keep_days = 31;
    let error = store
        .put_settings(&extending)
        .await
        .expect_err("growth should be refused during disk pressure");
    assert_eq!(
        error.to_string(),
        "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk"
    );

    let mut shrinking = previous;
    shrinking.retention_ladder.l2.keep_days = 20;
    let saved = store
        .put_settings(&shrinking)
        .await
        .expect("shrinking should remain allowed during disk pressure");
    assert_eq!(saved.retention_ladder.l2.keep_days, 20);
    assert_eq!(saved.rollup_retention_days, 20);
}

#[tokio::test]
async fn disk_pressure_refuses_enabling_a_tier_or_archive() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    let mut previous = DashboardSettings::default();
    previous.retention_ladder.l3.enabled = false;
    previous.retention_ladder.l4.enabled = false;
    let previous = store
        .put_settings(&previous)
        .await
        .expect("disabled baseline should save");
    set_disk_pressure(&store).await;

    let mut enabling_tier = previous.clone();
    enabling_tier.retention_ladder.l3.enabled = true;
    let tier_error = store
        .put_settings(&enabling_tier)
        .await
        .expect_err("enabling a tier should be refused");
    assert_eq!(
        tier_error.to_string(),
        "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk"
    );

    let mut enabling_archive = previous;
    enabling_archive.retention_ladder.archive.queryable = true;
    let archive_error = store
        .put_settings(&enabling_archive)
        .await
        .expect_err("enabling an archive should be refused");
    assert_eq!(
        archive_error.to_string(),
        "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk"
    );
}

#[tokio::test]
async fn settings_growth_and_a_breach_serialize_on_one_transaction() {
    // Break caught: settings growth commits after disk pressure becomes active because the
    // pressure read and settings write do not share one transaction.
    let (dir, database_url) = temp_database("settings-growth-disk-breach");
    let store = SqliteHistoryStore::connect(&database_url)
        .await
        .expect("store should connect");
    let observer = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("observer should connect");
    for statement in [
        "CREATE TABLE settings_pressure_order (sequence INTEGER PRIMARY KEY AUTOINCREMENT, operation TEXT NOT NULL)",
        "CREATE TRIGGER audit_dashboard_settings_insert AFTER INSERT ON app_settings WHEN NEW.setting_key = 'dashboard' BEGIN INSERT INTO settings_pressure_order (operation) VALUES ('settings'); END",
        "CREATE TRIGGER audit_dashboard_settings_update AFTER UPDATE OF value_json ON app_settings WHEN NEW.setting_key = 'dashboard' BEGIN INSERT INTO settings_pressure_order (operation) VALUES ('settings'); END",
        "CREATE TRIGGER audit_disk_pressure_insert AFTER INSERT ON history_state WHEN NEW.state_key = 'diskPressure' BEGIN INSERT INTO settings_pressure_order (operation) VALUES ('pressure'); END",
        "CREATE TRIGGER audit_disk_pressure_update AFTER UPDATE OF value_json ON history_state WHEN NEW.state_key = 'diskPressure' BEGIN INSERT INTO settings_pressure_order (operation) VALUES ('pressure'); END",
    ] {
        sqlx::query(statement)
            .execute(&observer)
            .await
            .expect("write-order observer should install");
    }

    let baseline = DashboardSettings::default();
    let mut grown = baseline.clone();
    grown.retention_ladder.l2.keep_days += 1;
    grown.rollup_retention_days += 1;
    let inactive = DiskPressureState {
        active: false,
        since_ms: None,
        free_bytes: 300,
        min_free_bytes: 200,
    };
    let mut breach_ladder = RetentionLadder::default();
    breach_ladder.disk_check.min_free_bytes = 200;
    let refusal = "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk";
    let mut settings_first = 0;
    let mut breach_first = 0;

    for iteration in 0..20 {
        store
            .put_settings(&baseline)
            .await
            .expect("baseline settings should save");
        store
            .history_state_set("diskPressure", &inactive, iteration)
            .await
            .expect("inactive pressure state should save");
        sqlx::query("DELETE FROM settings_pressure_order")
            .execute(&observer)
            .await
            .expect("write-order observer should reset");

        let (growth, breach) = tokio::join!(
            store.put_settings(&grown),
            apply_disk_measurement(&store, &dir, Ok(100), &breach_ladder, 1_000 + iteration,),
        );
        breach.expect("disk breach should commit");

        let stored = store
            .get_settings()
            .await
            .expect("stored settings should load");
        let pressure = store
            .history_state_get::<DiskPressureState>("diskPressure")
            .await
            .expect("pressure state should load")
            .expect("pressure state should exist");
        assert!(
            pressure.active,
            "iteration {iteration}: breach must be active"
        );
        let operations = sqlx::query_scalar::<_, String>(
            "SELECT operation FROM settings_pressure_order ORDER BY sequence",
        )
        .fetch_all(&observer)
        .await
        .expect("write order should load");

        match growth {
            Ok(saved) => {
                settings_first += 1;
                assert_eq!(saved, grown, "iteration {iteration}: saved growth");
                assert_eq!(stored, grown, "iteration {iteration}: stored growth");
                assert_eq!(
                    operations,
                    ["settings", "pressure"],
                    "iteration {iteration}: successful growth must commit before the breach",
                );
            }
            Err(StoreError::Validation(message)) if message == refusal => {
                breach_first += 1;
                assert_eq!(
                    stored, baseline,
                    "iteration {iteration}: refused growth must leave settings unchanged",
                );
                assert_eq!(
                    operations,
                    ["pressure"],
                    "iteration {iteration}: a prior breach must refuse growth before its write",
                );
            }
            result => panic!(
                "iteration {iteration}: expected committed growth or the exact disk-pressure refusal, observed {result:?}"
            ),
        }
    }

    println!(
        "observed orders across 20 iterations: settings-first={settings_first}, breach-first={breach_first}"
    );
    assert_eq!(settings_first + breach_first, 20);

    store.close().await.expect("store should close");
    observer.close().await;
    std::fs::remove_dir_all(dir).expect("owned temp fixture should be removable");
}

#[tokio::test]
async fn settings_save_persists_disabled_tier_flags_before_insert() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    store
        .history_state_set("l3FoldedUntilMs", &300_000_i64, 1)
        .await
        .expect("watermark should save");
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.l3.enabled = false;
    store
        .put_settings(&settings)
        .await
        .expect("settings should save");
    assert_eq!(
        store
            .history_state_get::<bool>("l3Enabled")
            .await
            .expect("flag should read"),
        Some(false)
    );

    store
        .insert_snapshot(60_000, &snapshot("2026-08-28T12:01:00Z", 25.0))
        .await
        .expect("insert should succeed before maintenance");
    let l3_rows = store
        .read_tier_buckets(Tier::L3, 0, 300_000)
        .await
        .expect("L3 rows should read");
    assert!(l3_rows.is_empty(), "disabled L3 must not be refolded");
}

#[tokio::test]
async fn settings_save_persists_disabled_l4_before_direct_l2_refold() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    store
        .history_state_set("l4FoldedUntilMs", &3_600_000_i64, 1)
        .await
        .expect("watermark should save");
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.l3.enabled = false;
    settings.retention_ladder.l4.enabled = false;
    store
        .put_settings(&settings)
        .await
        .expect("settings should save");
    assert_eq!(
        store
            .history_state_get::<bool>("l4Enabled")
            .await
            .expect("flag should read"),
        Some(false)
    );

    store
        .insert_snapshot(60_000, &snapshot("2026-08-28T12:01:00Z", 25.0))
        .await
        .expect("insert should succeed before maintenance");
    let l4_rows = store
        .read_tier_buckets(Tier::L4, 0, 3_600_000)
        .await
        .expect("L4 rows should read");
    assert!(l4_rows.is_empty(), "disabled L4 must not be refolded");
}

#[tokio::test]
async fn saved_detail_interval_controls_detail_row_cadence() {
    let store = SqliteHistoryStore::connect("sqlite::memory:")
        .await
        .expect("store should connect");
    let mut settings = DashboardSettings::default();
    settings.retention_ladder.detail_interval_sec = 15;
    let saved = store
        .put_settings(&settings)
        .await
        .expect("settings should save");

    store
        .insert_snapshot(1_000, &snapshot("2026-08-28T12:00:01Z", 10.0))
        .await
        .expect("first insert should succeed");
    tinytop_store::maintenance::maintain(&store, &saved, 1_000)
        .await
        .expect("first maintenance should clear pending rows");
    store
        .insert_snapshot(17_000, &snapshot("2026-08-28T12:00:17Z", 20.0))
        .await
        .expect("second insert should succeed");
    let report = tinytop_store::maintenance::maintain(&store, &saved, 17_000)
        .await
        .expect("second maintenance should report pending rows");

    assert_eq!(report.detail_rows, 2);
}

async fn set_disk_pressure(store: &SqliteHistoryStore) {
    store
        .history_state_set(
            "diskPressure",
            &json!({
                "active": true,
                "sinceMs": 1,
                "freeBytes": 100,
                "minFreeBytes": 200
            }),
            1,
        )
        .await
        .expect("disk pressure state should save");
}

fn temp_database(label: &str) -> (std::path::PathBuf, String) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tinytop-retention-{label}-{stamp}"));
    std::fs::create_dir_all(&dir).expect("temp directory should be created");
    let database_url = format!("sqlite://{}", dir.join("history.sqlite").display());
    (dir, database_url)
}

fn snapshot(timestamp: &str, cpu: f64) -> SystemSnapshot {
    SystemSnapshot {
        timestamp: timestamp.to_string(),
        identity: IdentitySnapshot {
            hostname: "devbox".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "Ubuntu 24.04.2 LTS".to_string(),
            kernel: "6.8.0-52-generic".to_string(),
            runtime: RuntimeDetection {
                kind: RuntimeKind::Linux,
                confidence: RuntimeConfidence::High,
                reason: "fixture".to_string(),
            },
            uptime_seconds: 60,
        },
        cpu: CpuSnapshot {
            usage_percent: cpu,
            cores: 4,
            times: CpuTimes::default(),
        },
        memory: MemorySnapshot {
            total_bytes: 100,
            available_bytes: 40,
            used_bytes: 60,
            used_percent: 60.0,
        },
        swap: SwapSnapshot {
            total_bytes: 10,
            free_bytes: 5,
            used_bytes: 5,
            used_percent: 50.0,
        },
        load: LoadSnapshot {
            one: 1.0,
            five: 2.0,
            fifteen: 3.0,
            runnable: 1,
            total_threads: 2,
            last_pid: 3,
        },
        pressure: PressureGroup {
            cpu: PressureSnapshot::default(),
            memory: PressureSnapshot::default(),
            io: PressureSnapshot::default(),
        },
        filesystems: vec![FilesystemSnapshot {
            filesystem: "/dev/sda1".to_string(),
            fs_type: "ext4".to_string(),
            size_bytes: 100,
            used_bytes: 50,
            available_bytes: 50,
            used_percent: 50.0,
            mount: "/".to_string(),
            inode_used_percent: Some(10.0),
            inode_used: Some(1),
            inode_total: Some(10),
        }],
        processes: vec![ProcessSnapshot {
            pid: 42,
            command: "tinytop".to_string(),
            cpu_percent: 1.0,
            memory_percent: 2.0,
            rss_bytes: 3,
            parent_pid: None,
            started_at: None,
        }],
    }
}
