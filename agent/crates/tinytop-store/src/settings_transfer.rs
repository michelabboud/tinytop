use serde::Serialize;
use serde_json::{Map, Value as JsonValue, json};
use time::OffsetDateTime;

use crate::{DashboardSettings, DiskPressureState, SqliteHistoryStore, StoreError, ladder::Tier};

pub const MAX_CONFIG_VERSION: i64 = 1;
pub const ENVELOPE_KEYS: [&str; 4] = [
    "tinytopConfigVersion",
    "exportedAtMs",
    "agentVersion",
    "settings",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDocument {
    pub tinytop_config_version: i64,
    pub exported_at_ms: i64,
    pub agent_version: String,
    pub settings: DashboardSettings,
}

pub fn export_document(
    settings: &DashboardSettings,
    now_ms: i64,
    agent_version: &str,
) -> SettingsDocument {
    SettingsDocument {
        tinytop_config_version: MAX_CONFIG_VERSION,
        exported_at_ms: now_ms,
        agent_version: agent_version.to_string(),
        settings: settings.clone(),
    }
}

pub fn export_filename(now_ms: i64) -> String {
    let Some(timestamp) = OffsetDateTime::from_unix_timestamp(now_ms.div_euclid(1_000)).ok() else {
        return "tinytop-settings.json".to_string();
    };
    format!(
        "tinytop-settings-{:04}{:02}{:02}-{:02}{:02}.json",
        timestamp.year(),
        u8::from(timestamp.month()),
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute()
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WouldDelete {
    pub l1_rows: i64,
    pub l2_buckets: i64,
    pub l3_buckets: i64,
    pub l4_buckets: i64,
    pub snapshot_json_rows: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub changed_keys: Vec<String>,
    pub would_delete: WouldDelete,
    #[serde(skip)]
    pub candidate: Option<DashboardSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub settings: DashboardSettings,
    pub changed_keys: Vec<String>,
    pub would_delete: WouldDelete,
}

pub async fn plan_import(
    store: &SqliteHistoryStore,
    document: &JsonValue,
    now_ms: i64,
) -> Result<ImportPlan, StoreError> {
    let Some(envelope) = document.as_object() else {
        return Ok(invalid_plan(vec![
            "configuration document must be a JSON object".to_string(),
        ]));
    };

    let envelope_errors = validate_envelope(envelope);
    if !envelope_errors.is_empty() {
        return Ok(invalid_plan(envelope_errors));
    }

    let previous = store.get_settings().await?;
    let disk_pressure = store
        .history_state_get::<DiskPressureState>("diskPressure")
        .await?
        .unwrap_or_default();
    let Some(settings_value) = envelope.get("settings") else {
        return Ok(invalid_plan(vec![
            "settings is required and must be an object".to_string(),
        ]));
    };
    let mut candidate = match DashboardSettings::from_document(
        settings_value.clone(),
        Some(&previous.retention_ladder),
    ) {
        Ok(candidate) => candidate,
        Err(error) => return Ok(invalid_plan(vec![error.to_string()])),
    };
    candidate.normalize_legacy_mirrors();

    let mut errors = Vec::new();
    if let Err(error) = candidate.validate() {
        errors.push(error.to_string());
    }
    if let Err(error) = candidate
        .retention_ladder
        .validate(Some(&disk_pressure), Some(&previous.retention_ladder))
    {
        errors.push(error.to_string());
    }
    if !errors.is_empty() {
        return Ok(invalid_plan(errors));
    }

    let serialized_candidate = serde_json::to_value(&candidate)?;
    let mut warnings = Vec::new();
    collect_unknown_settings_keys(
        settings_value,
        &serialized_candidate,
        "settings",
        &mut warnings,
    );
    warnings.sort();

    let changed_keys = DashboardSettings::changed_keys(&previous, &candidate)
        .into_iter()
        .map(str::to_string)
        .collect();
    let would_delete = would_delete(store, &candidate, now_ms).await?;

    Ok(ImportPlan {
        valid: true,
        errors: Vec::new(),
        warnings,
        changed_keys,
        would_delete,
        candidate: Some(candidate),
    })
}

pub async fn apply_import(
    store: &SqliteHistoryStore,
    document: &JsonValue,
    now_ms: i64,
) -> Result<ImportOutcome, StoreError> {
    let plan = plan_import(store, document, now_ms).await?;
    if !plan.valid {
        return Err(StoreError::Validation(plan.errors.join("; ")));
    }
    let Some(candidate) = plan.candidate.as_ref() else {
        return Err(StoreError::Validation(
            "settings import plan was valid but had no candidate".to_string(),
        ));
    };
    let settings = store.put_settings(candidate).await?;
    Ok(ImportOutcome {
        settings,
        changed_keys: plan.changed_keys,
        would_delete: plan.would_delete,
    })
}

pub fn import_marker(changed_keys: &[String]) -> (&'static str, JsonValue) {
    (
        "Settings imported",
        json!({"source": "import", "changed": changed_keys}),
    )
}

fn invalid_plan(errors: Vec<String>) -> ImportPlan {
    ImportPlan {
        valid: false,
        errors,
        warnings: Vec::new(),
        changed_keys: Vec::new(),
        would_delete: WouldDelete::default(),
        candidate: None,
    }
}

fn validate_envelope(envelope: &Map<String, JsonValue>) -> Vec<String> {
    let mut errors = envelope
        .keys()
        .filter(|key| !ENVELOPE_KEYS.contains(&key.as_str()))
        .map(|key| {
            format!(
                "unknown top-level key \"{key}\"; allowed keys: {}",
                ENVELOPE_KEYS.join(", ")
            )
        })
        .collect::<Vec<_>>();
    errors.sort();

    match envelope
        .get("tinytopConfigVersion")
        .and_then(JsonValue::as_i64)
    {
        None => errors.push(format!(
            "tinytopConfigVersion is required and must be an integer (maximum supported: {MAX_CONFIG_VERSION})"
        )),
        Some(version) if version > MAX_CONFIG_VERSION => errors.push(format!(
            "tinytopConfigVersion {version} is newer than the maximum supported {MAX_CONFIG_VERSION}; export from a matching agent or downgrade the document"
        )),
        Some(version) if version < 1 => errors.push(format!(
            "tinytopConfigVersion {version} must be ≥ 1 (maximum supported: {MAX_CONFIG_VERSION})"
        )),
        Some(_) => {}
    }
    if !envelope.get("settings").is_some_and(JsonValue::is_object) {
        errors.push("settings is required and must be an object".to_string());
    }
    errors
}

fn collect_unknown_settings_keys(
    input: &JsonValue,
    expected: &JsonValue,
    path: &str,
    warnings: &mut Vec<String>,
) {
    let (Some(input), Some(expected)) = (input.as_object(), expected.as_object()) else {
        return;
    };
    for (key, input_value) in input {
        let key_path = format!("{path}.{key}");
        match expected.get(key) {
            None => warnings.push(format!("{key_path}: unknown key ignored")),
            Some(expected_value) => {
                collect_unknown_settings_keys(input_value, expected_value, &key_path, warnings);
            }
        }
    }
}

async fn would_delete(
    store: &SqliteHistoryStore,
    candidate: &DashboardSettings,
    now_ms: i64,
) -> Result<WouldDelete, StoreError> {
    let config = candidate
        .retention_ladder
        .to_ladder_config(candidate.poll_interval_ms);
    let l1_rows = store
        .count_rows_older_than(Tier::L1, now_ms.saturating_sub(config.l1_keep_ms))
        .await?;
    let l2_buckets = store
        .count_rows_older_than(Tier::L2, now_ms.saturating_sub(config.l2_keep_ms))
        .await?;
    let l3_buckets = match config.l3 {
        Some(keep_ms) => {
            store
                .count_rows_older_than(Tier::L3, now_ms.saturating_sub(keep_ms))
                .await?
        }
        None => 0,
    };
    let l4_buckets = match config.l4.filter(|keep_ms| *keep_ms > 0) {
        Some(keep_ms) => {
            store
                .count_rows_older_than(Tier::L4, now_ms.saturating_sub(keep_ms))
                .await?
        }
        None => 0,
    };
    let snapshot_json_rows = store
        .count_snapshot_json_older_than(now_ms.saturating_sub(config.snapshot_json_keep_ms))
        .await?;
    Ok(WouldDelete {
        l1_rows,
        l2_buckets,
        l3_buckets,
        l4_buckets,
        snapshot_json_rows,
    })
}
