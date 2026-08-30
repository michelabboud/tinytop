use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value as JsonValue, json};
use sqlx::SqlitePool;
use tinytop_store::{
    DashboardSettings, DiskPressureState, FreeBytesProvider, SqliteHistoryStore, StoreError,
    check_disk,
    ladder::{Stat, Tier, TierBucket},
    maintenance::maintain_with_config,
    otel_settings::SECRET_SHAPED_KEY_WORDS,
    settings_transfer::{
        MAX_CONFIG_VERSION, apply_import, export_document, export_filename, import_marker,
        plan_import,
    },
};

const MINUTE_MS: i64 = 60_000;
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
            "tinytop-settings-transfer-{prefix}-{}-{stamp}",
            std::process::id()
        ));
        assert!(dir.starts_with(std::env::temp_dir()));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let url = format!("sqlite://{}", dir.join("history.sqlite").display());
        Self { dir, url }
    }

    async fn store(&self) -> SqliteHistoryStore {
        SqliteHistoryStore::connect(&self.url)
            .await
            .expect("fixture store should connect")
    }

    async fn pool(&self) -> SqlitePool {
        SqlitePool::connect(&self.url)
            .await
            .expect("fixture verification pool should connect")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

struct ScriptedFreeBytes(Mutex<VecDeque<Result<u64, io::ErrorKind>>>);

impl ScriptedFreeBytes {
    fn new(readings: impl IntoIterator<Item = Result<u64, io::ErrorKind>>) -> Self {
        Self(Mutex::new(readings.into_iter().collect()))
    }
}

impl FreeBytesProvider for ScriptedFreeBytes {
    fn free_bytes(&self, _path: &Path) -> io::Result<u64> {
        self.0
            .lock()
            .expect("scripted provider mutex should not be poisoned")
            .pop_front()
            .expect("scripted provider should have a reading")
            .map_err(|kind| io::Error::new(kind, "scripted free-bytes failure"))
    }
}

fn document(settings: &DashboardSettings) -> JsonValue {
    json!({
        "tinytopConfigVersion": MAX_CONFIG_VERSION,
        "exportedAtMs": 1_700_000_000_000_i64,
        "agentVersion": "9.9.9-test",
        "settings": settings,
    })
}

fn shrinking_candidate() -> DashboardSettings {
    let mut candidate = DashboardSettings::default();
    candidate.retention_ladder.l1.keep_days = 3;
    candidate.retention_ladder.l2.keep_days = 10;
    candidate.retention_ladder.l3.keep_days = 30;
    candidate.retention_ladder.l4.keep_days = 365;
    candidate
}

fn bucket(start_ms: i64, resolution_ms: i64) -> TierBucket {
    let stat = Stat {
        avg: 10.0,
        min: 10.0,
        max: 10.0,
    };
    TierBucket {
        bucket_start_ms: start_ms,
        first_captured_at_ms: start_ms,
        newest_captured_at_ms: start_ms + resolution_ms - 1,
        sample_count: 1,
        cpu: stat,
        memory: stat,
        swap: stat,
        load: stat,
        root_used: Some(stat),
    }
}

async fn seed_raw(pool: &SqlitePool, captured_at_ms: i64) {
    sqlx::query(
        r#"
        INSERT INTO metric_samples (
          captured_at_ms, snapshot_timestamp, hostname, runtime_kind,
          cpu_usage_percent, cpu_cores, memory_used_percent, memory_used_bytes,
          memory_total_bytes, swap_used_percent, swap_used_bytes, swap_total_bytes,
          load_one, load_five, load_fifteen, load_percent, runnable_threads,
          total_threads, root_used_percent
        ) VALUES (?, 'fixture', 'devbox', 'linux', 10, 4, 20, 20, 100,
                  0, 0, 0, 1, 1, 1, 25, 1, 4, 30)
        "#,
    )
    .bind(captured_at_ms)
    .execute(pool)
    .await
    .expect("raw fixture row should insert");
}

async fn seed_counts(store: &SqliteHistoryStore, pool: &SqlitePool, now_ms: i64) {
    seed_raw(pool, now_ms - 4 * DAY_MS).await;
    seed_raw(pool, now_ms - 2 * DAY_MS).await;
    seed_raw(pool, now_ms - 30 * MINUTE_MS).await;

    for (tier, old, recent) in [
        (Tier::L2, now_ms - 11 * DAY_MS, now_ms - 9 * DAY_MS),
        (Tier::L3, now_ms - 31 * DAY_MS, now_ms - 29 * DAY_MS),
        (Tier::L4, now_ms - 366 * DAY_MS, now_ms - 364 * DAY_MS),
    ] {
        store
            .upsert_tier_bucket(tier, &bucket(old, tier.resolution_ms()))
            .await
            .expect("old tier fixture should insert");
        store
            .upsert_tier_bucket(tier, &bucket(recent, tier.resolution_ms()))
            .await
            .expect("recent tier fixture should insert");
    }
}

async fn event_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM app_events")
        .fetch_one(pool)
        .await
        .expect("event count should read")
}

fn validation_message(error: StoreError) -> String {
    match error {
        StoreError::Validation(message) => message,
        other => panic!("expected validation error, got {other}"),
    }
}

#[tokio::test]
async fn export_document_carries_the_otel_block_and_still_no_secret_shaped_key() {
    // Break caught: exports omit envelope provenance or begin carrying credentials.
    let fixture = TempDatabase::new("export-envelope");
    let store = fixture.store().await;
    let settings = store.get_settings().await.expect("settings should read");
    let exported = export_document(&settings, 1_704_164_640_000, "9.9.9-test");
    let value = serde_json::to_value(exported).expect("export should serialize");

    assert_eq!(value["tinytopConfigVersion"], 1);
    assert_eq!(value["exportedAtMs"], 1_704_164_640_000_i64);
    assert_eq!(value["agentVersion"], "9.9.9-test");
    assert_eq!(
        value["settings"],
        serde_json::to_value(settings).expect("settings should serialize")
    );
    assert!(value["settings"]["otel"].is_object());
    assert_eq!(
        value["settings"]["otel"]["headersEnvVar"],
        "TINYTOP_OTEL_HEADERS"
    );

    fn assert_no_secret_key(value: &JsonValue, path: &str) {
        match value {
            JsonValue::Object(object) => {
                for (key, child) in object {
                    let lower = key.to_ascii_lowercase();
                    assert!(
                        SECRET_SHAPED_KEY_WORDS
                            .iter()
                            .all(|needle| !lower.contains(needle)),
                        "secret-shaped key exported at {path}.{key}"
                    );
                    assert_no_secret_key(child, &format!("{path}.{key}"));
                }
            }
            JsonValue::Array(values) => {
                for (index, child) in values.iter().enumerate() {
                    assert_no_secret_key(child, &format!("{path}[{index}]"));
                }
            }
            _ => {}
        }
    }
    assert_no_secret_key(&value, "$");
}

#[test]
fn export_filename_is_utc_minute_precise() {
    // Break caught: filenames depend on local time or lose minute precision.
    assert_eq!(
        export_filename(1_704_164_640_000),
        "tinytop-settings-20240102-0304.json"
    );
    assert_eq!(export_filename(i64::MAX), "tinytop-settings.json");
}

#[tokio::test]
async fn dry_run_reports_changed_keys_and_would_delete() {
    // Break caught: dry-run estimates counts or mutates settings/history.
    let fixture = TempDatabase::new("dry-run-counts");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let now_ms = 2_000 * DAY_MS;
    let mut current = DashboardSettings::default();
    current.retention_ladder.l1.keep_days = 5;
    store
        .put_settings(&current)
        .await
        .expect("current settings should save");
    seed_counts(&store, &pool, now_ms).await;

    let before = store.get_settings().await.expect("settings before dry-run");
    let plan = plan_import(&store, &document(&shrinking_candidate()), now_ms)
        .await
        .expect("dry-run should plan");

    assert!(plan.valid);
    assert_eq!(plan.changed_keys, ["retentionLadder"]);
    assert_eq!(plan.would_delete.l1_rows, 1);
    assert_eq!(plan.would_delete.l2_buckets, 1);
    assert_eq!(plan.would_delete.l3_buckets, 1);
    assert_eq!(plan.would_delete.l4_buckets, 1);
    assert_eq!(store.get_settings().await.unwrap(), before);
    assert_eq!(event_count(&pool).await, 0);
}

#[tokio::test]
async fn would_delete_counts_gpu_rows_at_the_l1_horizon() {
    // Break caught: a retention shrink deletes GPU samples without reporting them in the dry-run.
    let fixture = TempDatabase::new("gpu-dry-run-count");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let now_ms = 2_000 * DAY_MS;
    sqlx::query(
        "INSERT INTO gpu_adapters (stable_id, vendor, name, driver, first_seen_ms, last_seen_ms) VALUES ('pci-0000:02:00.0', 'amd', 'fixture', 'amdgpu', ?, ?)",
    )
    .bind(now_ms - 4 * DAY_MS)
    .bind(now_ms - 4 * DAY_MS)
    .execute(&pool)
    .await
    .expect("GPU adapter fixture should insert");
    sqlx::query(
        "INSERT INTO gpu_samples (captured_at_ms, adapter_id, busy_percent) SELECT ?, adapter_id, 25.0 FROM gpu_adapters WHERE stable_id = 'pci-0000:02:00.0'",
    )
    .bind(now_ms - 4 * DAY_MS)
    .execute(&pool)
    .await
    .expect("GPU sample fixture should insert");

    let plan = plan_import(&store, &document(&shrinking_candidate()), now_ms)
        .await
        .expect("GPU dry-run should plan");

    assert_eq!(plan.would_delete.gpu_sample_rows, 1);
}

#[tokio::test]
async fn import_plan_counts_fast_process_rows_for_a_shrinking_horizon() {
    // Break caught: shortening the fast-process horizon omits rows from the
    // dry-run, lengthening it invents deletions, or a 0.5.x document no longer
    // receives the 24-hour default cleanly.
    let fixture = TempDatabase::new("fast-process-horizon");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let now_ms = 2_000 * DAY_MS;
    sqlx::query("INSERT INTO process_commands (command) VALUES ('fixture-command')")
        .execute(&pool)
        .await
        .expect("command fixture should insert");
    let command_id: i64 = sqlx::query_scalar(
        "SELECT command_id FROM process_commands WHERE command = 'fixture-command'",
    )
    .fetch_one(&pool)
    .await
    .expect("command id should read");
    sqlx::query(
        "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, 1, 1, ?, 1.0, 2.0, 3, NULL, NULL, NULL)",
    )
    .bind(now_ms - 2 * 60 * MINUTE_MS)
    .bind(command_id)
    .execute(&pool)
    .await
    .expect("fast process fixture should insert");

    let mut shrinking = DashboardSettings::default();
    shrinking.retention_ladder.process_fast_keep_hours = 1;
    let shrink_plan = plan_import(&store, &document(&shrinking), now_ms)
        .await
        .expect("shrinking plan should compute");
    assert_eq!(shrink_plan.would_delete.process_fast_rows, 1);

    store
        .put_settings(&shrinking)
        .await
        .expect("short horizon should save");
    sqlx::query(
        "INSERT INTO process_samples_fast (captured_at_ms, rank, pid, command_id, cpu_percent, memory_percent, rss_bytes, parent_pid, started_at_ms, gpu_percent) VALUES (?, 2, 2, ?, 1.0, 2.0, 3, NULL, NULL, NULL)",
    )
    .bind(now_ms - 30 * 60 * MINUTE_MS)
    .bind(command_id)
    .execute(&pool)
    .await
    .expect("older fast process fixture should insert");
    let growing = DashboardSettings::default();
    let unpruned_growth_plan = plan_import(&store, &document(&growing), now_ms)
        .await
        .expect("unpruned growing plan should compute");
    assert_eq!(unpruned_growth_plan.would_delete.process_fast_rows, 1);

    let config = shrinking
        .retention_ladder
        .to_ladder_config(shrinking.poll_interval_ms);
    let report = maintain_with_config(&store, &config, now_ms)
        .await
        .expect("maintenance should prune fast process history");
    assert_eq!(report.process_fast_rows, 2);
    let growth_plan = plan_import(&store, &document(&growing), now_ms)
        .await
        .expect("growing plan should compute");
    assert_eq!(growth_plan.would_delete.process_fast_rows, 0);

    let mut legacy_settings = serde_json::to_value(DashboardSettings::default())
        .expect("legacy settings should serialize");
    legacy_settings["retentionLadder"]
        .as_object_mut()
        .expect("retention ladder should be an object")
        .remove("processFastKeepHours");
    let legacy_plan = plan_import(
        &store,
        &json!({"tinytopConfigVersion": 1, "settings": legacy_settings}),
        now_ms,
    )
    .await
    .expect("0.5.x document should plan");
    assert!(legacy_plan.valid);
    assert_eq!(
        legacy_plan
            .candidate
            .expect("valid legacy plan should carry a candidate")
            .retention_ladder
            .process_fast_keep_hours,
        24
    );
    assert!(
        legacy_plan
            .warnings
            .iter()
            .all(|warning| !warning.contains("processFastKeepHours"))
    );
    pool.close().await;
}

#[tokio::test]
async fn dry_run_honors_exact_prune_predicate_boundaries() {
    // Break caught: count previews drift from `<` for raw or `<=` for rollup ends.
    let fixture = TempDatabase::new("predicate-boundaries");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let now_ms = 2_000 * DAY_MS;
    let candidate = shrinking_candidate();
    let ladder = &candidate.retention_ladder;

    let l1_cutoff = now_ms - ladder.l1.keep_days * DAY_MS;
    seed_raw(&pool, l1_cutoff).await;

    for (tier, keep_days) in [
        (Tier::L2, ladder.l2.keep_days),
        (Tier::L3, ladder.l3.keep_days),
        (Tier::L4, ladder.l4.keep_days),
    ] {
        let cutoff = now_ms - keep_days * DAY_MS;
        let start = cutoff - tier.resolution_ms();
        store
            .upsert_tier_bucket(tier, &bucket(start, tier.resolution_ms()))
            .await
            .expect("boundary tier fixture should insert");
        assert_eq!(start + tier.resolution_ms(), cutoff);
    }

    let plan = plan_import(&store, &document(&candidate), now_ms)
        .await
        .expect("boundary dry-run should plan");

    assert!(plan.valid);
    assert_eq!(
        plan.would_delete.l1_rows, 0,
        "L1 uses captured_at_ms < cutoff"
    );
    assert_eq!(
        plan.would_delete.l2_buckets, 1,
        "L2 uses bucket end <= cutoff"
    );
    assert_eq!(
        plan.would_delete.l3_buckets, 1,
        "L3 uses bucket end <= cutoff"
    );
    assert_eq!(
        plan.would_delete.l4_buckets, 1,
        "L4 uses bucket end <= cutoff"
    );
}

#[tokio::test]
async fn dry_run_counts_zero_for_a_disabled_tier_and_a_forever_l4() {
    // Break caught: rows retained by disabled/forever tiers are presented as deletions.
    let fixture = TempDatabase::new("disabled-forever");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let now_ms = 2_000 * DAY_MS;
    seed_counts(&store, &pool, now_ms).await;
    let mut candidate = shrinking_candidate();
    candidate.retention_ladder.l3.enabled = false;
    candidate.retention_ladder.l4.keep_days = 0;

    let plan = plan_import(&store, &document(&candidate), now_ms)
        .await
        .expect("dry-run should plan");

    assert!(plan.valid);
    assert_eq!(plan.would_delete.l3_buckets, 0);
    assert_eq!(plan.would_delete.l4_buckets, 0);
}

#[tokio::test]
async fn dry_run_reports_moved_rows_for_a_queryable_archive_candidate() {
    // Break caught: queryable archive hides the L4 rows that leave the main database.
    let fixture = TempDatabase::new("archive-moves");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let now_ms = 2_000 * DAY_MS;
    seed_counts(&store, &pool, now_ms).await;
    let mut candidate = shrinking_candidate();
    candidate.retention_ladder.archive.queryable = true;

    let plan = plan_import(&store, &document(&candidate), now_ms)
        .await
        .expect("dry-run should plan");

    assert!(plan.valid);
    assert_eq!(plan.would_delete.l4_buckets, 1);
}

#[tokio::test]
async fn import_applies_normalises_mirrors_and_returns_the_outcome() {
    // Break caught: import bypasses put_settings normalization or records its own marker.
    let fixture = TempDatabase::new("apply");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let mut candidate = DashboardSettings::default();
    candidate.retention_ladder.l1.keep_days = 4;
    candidate.retention_ladder.l2.keep_days = 14;
    candidate.retention_hours = 999;
    candidate.rollup_retention_days = 999;

    let outcome = apply_import(&store, &document(&candidate), 2_000 * DAY_MS)
        .await
        .expect("import should apply");

    assert_eq!(outcome.settings.retention_hours, 96);
    assert_eq!(outcome.settings.rollup_retention_days, 14);
    assert_eq!(store.get_settings().await.unwrap(), outcome.settings);
    assert_eq!(
        store.history_state_get::<bool>("l3Enabled").await.unwrap(),
        Some(true)
    );
    assert_eq!(
        store.history_state_get::<bool>("l4Enabled").await.unwrap(),
        Some(true)
    );
    assert_eq!(event_count(&pool).await, 0);
}

#[test]
fn import_marker_details_name_the_source_and_the_keys() {
    // Break caught: import markers become indistinguishable from ordinary PUT markers.
    let changed = vec!["defaultTheme".to_string(), "retentionLadder".to_string()];
    let (label, details) = import_marker(&changed);
    assert_eq!(label, "Settings imported");
    assert_eq!(
        details,
        json!({"source": "import", "changed": ["defaultTheme", "retentionLadder"]})
    );
}

#[tokio::test]
async fn unknown_top_level_key_is_refused_and_nothing_is_written() {
    // Break caught: an unsupported envelope extension is silently accepted.
    let fixture = TempDatabase::new("unknown-envelope");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let before = store.get_settings().await.unwrap();
    let before_l3_enabled = store.history_state_get::<bool>("l3Enabled").await.unwrap();
    let before_l4_enabled = store.history_state_get::<bool>("l4Enabled").await.unwrap();
    let mut input = document(&before);
    input["zzz"] = json!(true);
    input["aaa"] = json!(false);

    let plan = plan_import(&store, &input, 0).await.unwrap();
    assert!(!plan.valid);
    assert_eq!(
        plan.errors,
        [
            "unknown top-level key \"aaa\"; allowed keys: tinytopConfigVersion, exportedAtMs, agentVersion, settings",
            "unknown top-level key \"zzz\"; allowed keys: tinytopConfigVersion, exportedAtMs, agentVersion, settings",
        ]
    );
    assert!(plan.candidate.is_none());
    assert!(matches!(
        apply_import(&store, &input, 0).await,
        Err(StoreError::Validation(_))
    ));
    assert_eq!(store.get_settings().await.unwrap(), before);
    assert_eq!(event_count(&pool).await, 0);
    assert_eq!(
        store.history_state_get::<bool>("l3Enabled").await.unwrap(),
        before_l3_enabled
    );
    assert_eq!(
        store.history_state_get::<bool>("l4Enabled").await.unwrap(),
        before_l4_enabled
    );
}

#[tokio::test]
async fn newer_config_version_is_refused_naming_the_max() {
    // Break caught: unsupported, missing, and pre-v1 envelopes reach the decoder.
    let fixture = TempDatabase::new("versions");
    let store = fixture.store().await;
    let pool = fixture.pool().await;
    let settings = store.get_settings().await.unwrap();
    let before_l3_enabled = store.history_state_get::<bool>("l3Enabled").await.unwrap();
    let before_l4_enabled = store.history_state_get::<bool>("l4Enabled").await.unwrap();

    let mut newer = document(&settings);
    newer["tinytopConfigVersion"] = json!(2);
    assert_eq!(
        plan_import(&store, &newer, 0).await.unwrap().errors,
        [
            "tinytopConfigVersion 2 is newer than the maximum supported 1; export from a matching agent or downgrade the document"
        ]
    );

    let mut missing = document(&settings);
    missing
        .as_object_mut()
        .unwrap()
        .remove("tinytopConfigVersion");
    assert_eq!(
        plan_import(&store, &missing, 0).await.unwrap().errors,
        ["tinytopConfigVersion is required and must be an integer (maximum supported: 1)"]
    );

    for invalid_version in [json!("1"), json!(1.5)] {
        let mut wrong_type = document(&settings);
        wrong_type["tinytopConfigVersion"] = invalid_version;
        let plan = plan_import(&store, &wrong_type, 0).await.unwrap();
        assert!(!plan.valid);
        assert_eq!(
            plan.errors,
            ["tinytopConfigVersion is required and must be an integer (maximum supported: 1)"]
        );
    }

    let mut old = document(&settings);
    old["tinytopConfigVersion"] = json!(0);
    assert_eq!(
        plan_import(&store, &old, 0).await.unwrap().errors,
        ["tinytopConfigVersion 0 must be ≥ 1 (maximum supported: 1)"]
    );

    assert!(matches!(
        apply_import(&store, &newer, 0).await,
        Err(StoreError::Validation(_))
    ));
    assert_eq!(store.get_settings().await.unwrap(), settings);
    assert_eq!(event_count(&pool).await, 0);
    assert_eq!(
        store.history_state_get::<bool>("l3Enabled").await.unwrap(),
        before_l3_enabled
    );
    assert_eq!(
        store.history_state_get::<bool>("l4Enabled").await.unwrap(),
        before_l4_enabled
    );
}

#[tokio::test]
async fn unknown_keys_inside_settings_are_warnings_not_errors() {
    // Break caught: forward-compatible settings extensions are refused or ignored silently.
    let fixture = TempDatabase::new("settings-warnings");
    let store = fixture.store().await;
    let settings = store.get_settings().await.unwrap();
    let mut input = document(&settings);
    input["settings"]["bogus"] = json!(true);
    input["settings"]["retentionLadder"]["l9"] = json!({"keepDays": 1});
    input["settings"]["retentionLadder"]["snapshotJsonKeepMinutes"] = json!(59);

    let plan = plan_import(&store, &input, 0).await.unwrap();

    assert!(plan.valid);
    assert!(plan.errors.is_empty());
    assert_eq!(
        plan.warnings,
        [
            "settings.bogus: unknown key ignored",
            "settings.retentionLadder.l9: unknown key ignored",
            "snapshotJsonKeepMinutes is no longer used and was ignored",
        ]
    );
}

#[tokio::test]
async fn removed_keep_minutes_is_ignored_while_the_document_applies() {
    // Break caught: 0.5.2 documents are refused, retain the retired key, emit
    // duplicate warnings, or discard supported settings beside the old key.
    let fixture = TempDatabase::new("legacy-snapshot-json-setting");
    let store = fixture.store().await;
    let mut candidate = DashboardSettings::default();
    candidate.retention_ladder.l1.keep_days = 4;
    let mut input = document(&candidate);
    input["settings"]["retentionLadder"]["snapshotJsonKeepMinutes"] = json!(60);

    let plan = plan_import(&store, &input, 0)
        .await
        .expect("legacy document should plan");
    assert!(plan.valid);
    assert_eq!(
        plan.warnings,
        ["snapshotJsonKeepMinutes is no longer used and was ignored"]
    );
    assert_eq!(
        plan.candidate
            .as_ref()
            .expect("valid plan should carry a candidate")
            .retention_ladder
            .l1
            .keep_days,
        4
    );

    let outcome = apply_import(&store, &input, 0)
        .await
        .expect("legacy document should apply");
    assert_eq!(outcome.settings.retention_ladder.l1.keep_days, 4);
    let stored = serde_json::to_value(store.get_settings().await.expect("stored settings"))
        .expect("stored settings should serialize");
    assert!(
        stored["retentionLadder"]
            .get("snapshotJsonKeepMinutes")
            .is_none()
    );

    let exported = serde_json::to_value(export_document(&outcome.settings, 0, "test"))
        .expect("export should serialize");
    assert!(
        exported["settings"]["retentionLadder"]
            .get("snapshotJsonKeepMinutes")
            .is_none()
    );
}

#[tokio::test]
async fn import_under_disk_pressure_refuses_growth_and_allows_shrink() {
    // Break caught: import bypasses disk-pressure growth refusal or blocks safe shrinkage.
    let fixture = TempDatabase::new("pressure");
    let store = fixture.store().await;
    let mut current = DashboardSettings::default();
    current.retention_ladder.l1.keep_days = 5;
    let current = store.put_settings(&current).await.unwrap();

    let mut pressure_ladder = current.retention_ladder.clone();
    pressure_ladder.disk_check.min_free_bytes = 200;
    check_disk(
        &store,
        &ScriptedFreeBytes::new([Ok(100)]),
        &pressure_ladder,
        10,
    )
    .await
    .expect("scripted pressure check should succeed");
    assert_eq!(
        store
            .history_state_get::<DiskPressureState>("diskPressure")
            .await
            .unwrap()
            .unwrap()
            .free_bytes,
        100
    );

    let mut growth = current.clone();
    growth.retention_ladder.l1.keep_days = 6;
    let growth_plan = plan_import(&store, &document(&growth), 20).await.unwrap();
    let expected = "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk";
    assert!(!growth_plan.valid);
    assert_eq!(growth_plan.errors, [expected]);
    assert_eq!(
        validation_message(
            apply_import(&store, &document(&growth), 20)
                .await
                .unwrap_err()
        ),
        expected
    );
    assert_eq!(store.get_settings().await.unwrap(), current);

    let mut shrink = current.clone();
    shrink.retention_ladder.l1.keep_days = 3;
    let outcome = apply_import(&store, &document(&shrink), 20)
        .await
        .expect("shrink should apply under pressure");
    assert_eq!(outcome.settings.retention_ladder.l1.keep_days, 3);
}

#[tokio::test]
async fn pressure_arriving_between_plan_and_apply_is_still_refused() {
    // Break caught: apply trusts an earlier preview instead of authoritative put_settings validation.
    let fixture = TempDatabase::new("pressure-race");
    let store = fixture.store().await;
    let mut growth = DashboardSettings::default();
    growth.retention_ladder.l1.keep_days = 4;
    assert!(
        plan_import(&store, &document(&growth), 0)
            .await
            .unwrap()
            .valid
    );

    let mut pressure_ladder = DashboardSettings::default().retention_ladder;
    pressure_ladder.disk_check.min_free_bytes = 200;
    check_disk(
        &store,
        &ScriptedFreeBytes::new([Ok(100)]),
        &pressure_ladder,
        10,
    )
    .await
    .unwrap();

    assert_eq!(
        validation_message(
            apply_import(&store, &document(&growth), 20)
                .await
                .unwrap_err()
        ),
        "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk"
    );
    assert_eq!(
        store.get_settings().await.unwrap(),
        DashboardSettings::default()
    );
}

#[tokio::test]
async fn legacy_document_without_a_ladder_is_decoded_through_from_document() {
    // Break caught: import deserializes directly and discards legacy alias derivation/persisted tiers.
    let fixture = TempDatabase::new("legacy");
    let store = fixture.store().await;
    let persisted = store.get_settings().await.unwrap();
    let mut legacy = serde_json::to_value(&persisted).unwrap();
    legacy.as_object_mut().unwrap().remove("retentionLadder");
    legacy["retentionHours"] = json!(96);
    legacy["rollupRetentionDays"] = json!(14);

    let plan = plan_import(
        &store,
        &json!({"tinytopConfigVersion": 1, "settings": legacy}),
        0,
    )
    .await
    .unwrap();

    assert!(plan.valid);
    let candidate = plan.candidate.expect("valid plans carry a candidate");
    assert_eq!(candidate.retention_ladder.l1.keep_days, 4);
    assert_eq!(candidate.retention_ladder.l2.keep_days, 14);
    assert_eq!(candidate.retention_ladder.l3, persisted.retention_ladder.l3);
    assert_eq!(candidate.retention_ladder.l4, persisted.retention_ladder.l4);
}

#[tokio::test]
async fn import_without_otel_keeps_the_persisted_block() {
    // Break caught: a 0.4.1 settings import disables or rewrites live OTel export.
    let fixture = TempDatabase::new("legacy-without-otel");
    let store = fixture.store().await;
    let mut persisted = DashboardSettings::default();
    persisted.otel.enabled = true;
    persisted.otel.endpoint = "https://collector.example/v1/metrics".to_string();
    persisted
        .otel
        .resource_attributes
        .insert("deployment.environment".to_string(), "test".to_string());
    let persisted = store
        .put_settings(&persisted)
        .await
        .expect("OTel settings should save");
    let mut legacy_settings = serde_json::to_value(&persisted).unwrap();
    legacy_settings.as_object_mut().unwrap().remove("otel");
    let input = json!({
        "tinytopConfigVersion": 1,
        "settings": legacy_settings,
    });

    let plan = plan_import(&store, &input, 0).await.unwrap();

    assert!(plan.valid);
    assert!(
        plan.warnings
            .iter()
            .all(|warning| !warning.contains("otel"))
    );
    assert!(plan.changed_keys.iter().all(|key| key != "otel"));
    assert_eq!(plan.candidate.as_ref().unwrap().otel, persisted.otel);

    let outcome = apply_import(&store, &input, 0)
        .await
        .expect("legacy import should apply");
    assert_eq!(outcome.settings.otel, persisted.otel);
    assert_eq!(store.get_settings().await.unwrap().otel, persisted.otel);
}

#[tokio::test]
async fn import_with_an_invalid_otel_block_is_refused() {
    // Break caught: invalid endpoints are accepted by dry-run or partially persisted.
    let fixture = TempDatabase::new("invalid-otel");
    let store = fixture.store().await;
    let before = store.get_settings().await.unwrap();
    let mut candidate = before.clone();
    candidate.otel.endpoint = "collector:4318/v1/metrics".to_string();

    let plan = plan_import(&store, &document(&candidate), 0).await.unwrap();

    assert!(!plan.valid);
    assert_eq!(
        plan.errors,
        ["otel.endpoint must be an http:// or https:// URL with a host and without credentials"]
    );
    assert_eq!(
        validation_message(
            apply_import(&store, &document(&candidate), 0)
                .await
                .unwrap_err()
        ),
        "otel.endpoint must be an http:// or https:// URL with a host and without credentials"
    );
    assert_eq!(store.get_settings().await.unwrap(), before);
}

#[tokio::test]
async fn put_settings_document_merges_against_the_in_transaction_previous() {
    // Break caught: document decoding before BEGIN IMMEDIATE captures stale
    // settings and reverts an OTel block that the document never carried.
    let fixture = TempDatabase::new("settings-document-transactional-merge");
    let store = fixture.store().await;
    let mut enabled_document =
        serde_json::to_value(DashboardSettings::default()).expect("settings should serialize");
    enabled_document["otel"]["enabled"] = json!(true);
    let enabled = store
        .put_settings_document(&enabled_document)
        .await
        .expect("document should enable OTel");
    assert!(enabled.saved.otel.enabled);

    let mut legacy_document =
        serde_json::to_value(&enabled.saved).expect("settings should serialize");
    legacy_document.as_object_mut().unwrap().remove("otel");
    let write = store
        .put_settings_document(&legacy_document)
        .await
        .expect("legacy document should merge and save");

    assert!(write.previous.otel.enabled);
    assert!(write.saved.otel.enabled);
    assert!(!DashboardSettings::changed_keys(&write.previous, &write.saved).contains(&"otel"));
}

#[tokio::test]
async fn put_settings_document_refuses_an_invalid_document_and_writes_nothing() {
    // Break caught: document validation happens after persistence or leaves a
    // partial settings write behind on refusal.
    let fixture = TempDatabase::new("settings-document-invalid");
    let store = fixture.store().await;
    let before = store.get_settings().await.expect("default settings");
    let mut document = serde_json::to_value(&before).expect("settings should serialize");
    document["otel"]["endpoint"] = json!("http://:4318/v1/metrics");

    let error = store
        .put_settings_document(&document)
        .await
        .expect_err("invalid endpoint should be refused");

    assert_eq!(
        validation_message(error),
        "otel.endpoint must be an http:// or https:// URL with a host and without credentials"
    );
    assert_eq!(store.get_settings().await.expect("stored settings"), before);
}

#[test]
fn changed_keys_reports_the_ladder_once_instead_of_derived_aliases() {
    // Break caught: a ladder edit is reported as derived aliases or duplicated keys.
    let previous = DashboardSettings::default();
    let mut saved = previous.clone();
    saved.retention_ladder.l1.keep_days = 4;
    saved.retention_hours = 96;

    assert_eq!(
        DashboardSettings::changed_keys(&previous, &saved),
        ["retentionLadder"]
    );
}
