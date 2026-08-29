use std::{
    collections::VecDeque,
    io,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use sqlx::{Row, SqlitePool};
use tinytop_store::{
    DashboardSettings, DiskCheckReport, DiskPressureState, DiskTransition, FreeBytesProvider,
    SqliteHistoryStore, StoreError, SysinfoFreeBytes, check_disk,
    retention_ladder::RetentionLadder,
};

struct TempDatabase {
    dir: std::path::PathBuf,
    url: String,
}

impl TempDatabase {
    fn new(prefix: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tinytop-disk-check-{prefix}-{}-{stamp}",
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

fn ladder_with_min_free_bytes(min_free_bytes: i64) -> RetentionLadder {
    RetentionLadder {
        disk_check: tinytop_store::retention_ladder::DiskCheckSettings {
            min_free_bytes,
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn pressure_state(store: &SqliteHistoryStore) -> DiskPressureState {
    store
        .history_state_get("diskPressure")
        .await
        .expect("disk-pressure state should read")
        .expect("disk-pressure state should exist")
}

async fn last_check_ms(store: &SqliteHistoryStore) -> Option<i64> {
    store
        .history_state_get("lastDiskCheckMs")
        .await
        .expect("last disk-check time should read")
}

async fn state_json(pool: &SqlitePool) -> String {
    sqlx::query_scalar("SELECT value_json FROM history_state WHERE state_key = 'diskPressure'")
        .fetch_one(pool)
        .await
        .expect("raw disk-pressure state should read")
}

async fn app_events(pool: &SqlitePool) -> Vec<(i64, String, String, Value)> {
    sqlx::query(
        "SELECT occurred_at_ms, marker_type, label, details_json FROM app_events ORDER BY rowid",
    )
    .fetch_all(pool)
    .await
    .expect("app events should read")
    .into_iter()
    .map(|row| {
        let details_json: String = row
            .try_get("details_json")
            .expect("details JSON should read");
        (
            row.try_get("occurred_at_ms")
                .expect("event timestamp should read"),
            row.try_get("marker_type")
                .expect("event marker type should read"),
            row.try_get("label").expect("event label should read"),
            serde_json::from_str(&details_json).expect("event details should be JSON"),
        )
    })
    .collect()
}

fn assert_breach_report(report: &DiskCheckReport, path: &Path, free_bytes: i64) {
    assert_eq!(report.path, path);
    assert_eq!(report.free_bytes, free_bytes);
    assert_eq!(report.min_free_bytes, 200);
    assert!(report.database_bytes > 0);
    assert!(report.pressure);
    assert_eq!(report.transition, DiskTransition::Breached);
}

#[tokio::test]
async fn breach_activates_pressure_and_records_one_marker() {
    // Break caught: a first breach fails to atomically persist state, time, and marker details.
    let fixture = TempDatabase::new("breach");
    let store = fixture.store().await;
    let provider = ScriptedFreeBytes::new([Ok(100)]);
    let ladder = ladder_with_min_free_bytes(200);
    let now1 = 1_000;

    let report = check_disk(&store, &provider, &ladder, now1)
        .await
        .expect("breach check should succeed");

    assert_breach_report(&report, &fixture.dir, 100);
    assert_eq!(
        pressure_state(&store).await,
        DiskPressureState {
            active: true,
            since_ms: Some(now1),
            free_bytes: 100,
            min_free_bytes: 200,
        }
    );
    assert_eq!(last_check_ms(&store).await, Some(now1));
    let events = app_events(&fixture.pool().await).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, now1);
    assert_eq!(events[0].1, "diskPressure");
    assert_eq!(events[0].2, "Disk pressure: free 100 < minFreeBytes 200");
    assert_eq!(events[0].3["freeBytes"], 100);
    assert_eq!(events[0].3["minFreeBytes"], 200);
    assert!(events[0].3["databaseBytes"].as_i64().unwrap_or_default() > 0);
    assert_eq!(events[0].3["path"], fixture.dir.display().to_string());
}

#[tokio::test]
async fn repeated_breach_updates_free_bytes_but_not_since_or_markers() {
    // Break caught: a continuing breach resets sinceMs or emits duplicate pressure markers.
    let fixture = TempDatabase::new("repeated-breach");
    let store = fixture.store().await;
    let provider = ScriptedFreeBytes::new([Ok(100), Ok(90)]);
    let ladder = ladder_with_min_free_bytes(200);
    let now1 = 1_000;
    let now2 = 2_000;
    check_disk(&store, &provider, &ladder, now1)
        .await
        .expect("first breach should succeed");

    let report = check_disk(&store, &provider, &ladder, now2)
        .await
        .expect("repeated breach should succeed");

    assert_eq!(report.transition, DiskTransition::Unchanged);
    assert!(report.pressure);
    assert_eq!(report.free_bytes, 90);
    assert_eq!(
        pressure_state(&store).await,
        DiskPressureState {
            active: true,
            since_ms: Some(now1),
            free_bytes: 90,
            min_free_bytes: 200,
        }
    );
    assert_eq!(last_check_ms(&store).await, Some(now2));
    assert_eq!(app_events(&fixture.pool().await).await.len(), 1);
}

#[tokio::test]
async fn recovery_deactivates_and_records_recovered_marker() {
    // Break caught: recovery leaves pressure active or omits/orders its marker incorrectly.
    let fixture = TempDatabase::new("recovery");
    let store = fixture.store().await;
    let provider = ScriptedFreeBytes::new([Ok(100), Ok(300)]);
    let ladder = ladder_with_min_free_bytes(200);
    let now1 = 1_000;
    let now2 = 2_000;
    check_disk(&store, &provider, &ladder, now1)
        .await
        .expect("breach should succeed");

    let report = check_disk(&store, &provider, &ladder, now2)
        .await
        .expect("recovery should succeed");

    assert_eq!(report.transition, DiskTransition::Recovered);
    assert!(!report.pressure);
    assert_eq!(
        pressure_state(&store).await,
        DiskPressureState {
            active: false,
            since_ms: None,
            free_bytes: 300,
            min_free_bytes: 200,
        }
    );
    let events = app_events(&fixture.pool().await).await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.1.as_str())
            .collect::<Vec<_>>(),
        ["diskPressure", "diskRecovered"]
    );
    assert_eq!(events[1].0, now2);
    assert_eq!(
        events[1].2,
        "Disk pressure cleared: free 300 ≥ minFreeBytes 200"
    );
}

#[tokio::test]
async fn healthy_check_refreshes_free_bytes_without_markers() {
    // Break caught: a healthy first check fails to refresh coverage or invents a transition.
    let fixture = TempDatabase::new("healthy");
    let store = fixture.store().await;
    let provider = ScriptedFreeBytes::new([Ok(300)]);
    let ladder = ladder_with_min_free_bytes(200);
    let now1 = 1_000;

    let report = check_disk(&store, &provider, &ladder, now1)
        .await
        .expect("healthy check should succeed");

    assert_eq!(report.transition, DiskTransition::Unchanged);
    assert!(!report.pressure);
    assert_eq!(
        pressure_state(&store).await,
        DiskPressureState {
            active: false,
            since_ms: None,
            free_bytes: 300,
            min_free_bytes: 200,
        }
    );
    assert_eq!(last_check_ms(&store).await, Some(now1));
    assert!(app_events(&fixture.pool().await).await.is_empty());
}

#[tokio::test]
async fn undeterminable_free_bytes_keeps_state_and_returns_error() {
    // Break caught: an I/O failure mutates/clears the last known pressure state or check time.
    let fixture = TempDatabase::new("undeterminable");
    let store = fixture.store().await;
    let provider = ScriptedFreeBytes::new([Ok(100), Err(io::ErrorKind::NotFound)]);
    let ladder = ladder_with_min_free_bytes(200);
    let now1 = 1_000;
    let now2 = 2_000;
    check_disk(&store, &provider, &ladder, now1)
        .await
        .expect("breach should succeed");
    let pool = fixture.pool().await;
    let before = state_json(&pool).await;

    let error = check_disk(&store, &provider, &ladder, now2)
        .await
        .expect_err("undeterminable measurement should fail");

    match &error {
        StoreError::DiskCheck { path, source } => {
            assert_eq!(path, &fixture.dir);
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("expected StoreError::DiskCheck, got {other:?}"),
    }
    let message = error.to_string();
    assert!(message.contains(&fixture.dir.display().to_string()));
    assert!(message.contains("last known"));
    assert_eq!(state_json(&pool).await, before);
    assert_eq!(last_check_ms(&store).await, Some(now1));
    assert_eq!(app_events(&pool).await.len(), 1);
}

#[tokio::test]
async fn pressure_from_the_check_refuses_growth_and_allows_shrink() {
    // Break caught: persisted pressure from the real check path does not gate settings growth.
    // The exhaustive validation matrix remains disk_pressure_rule_table in retention_settings.rs.
    let fixture = TempDatabase::new("growth-refusal");
    let store = fixture.store().await;
    let baseline = store
        .put_settings(&DashboardSettings::default())
        .await
        .expect("baseline settings should save");
    let provider = ScriptedFreeBytes::new([Ok(100), Ok(300)]);
    let ladder = ladder_with_min_free_bytes(200);
    check_disk(&store, &provider, &ladder, 1_000)
        .await
        .expect("breach should succeed");

    let mut growing = baseline.clone();
    growing.retention_ladder.l2.keep_days = 31;
    let error = store
        .put_settings(&growing)
        .await
        .expect_err("growth should be refused under measured pressure");
    assert_eq!(
        error.to_string(),
        "disk pressure active: free 100 < minFreeBytes 200; shrink first or free disk"
    );

    let mut shrinking = baseline;
    shrinking.retention_ladder.l2.keep_days = 20;
    let shrunk = store
        .put_settings(&shrinking)
        .await
        .expect("shrink should be allowed under pressure");

    check_disk(&store, &provider, &ladder, 2_000)
        .await
        .expect("recovery should succeed");
    let mut growing_after_recovery = shrunk;
    growing_after_recovery.retention_ladder.l2.keep_days = 31;
    store
        .put_settings(&growing_after_recovery)
        .await
        .expect("growth should be allowed after recovery");
}

#[test]
fn sysinfo_provider_measures_a_real_directory() {
    // Break caught: the real provider cannot resolve a normal temp directory's mount.
    let fixture = TempDatabase::new("sysinfo");

    let free_bytes = SysinfoFreeBytes
        .free_bytes(&fixture.dir)
        .expect("temp directory free bytes should be measurable");

    assert!(free_bytes > 0);
}
