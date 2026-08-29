use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{StoreError, maintenance::LadderConfig};

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;
const MIN_FREE_BYTES: i64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RetentionLadder {
    #[serde(default = "default_l1", deserialize_with = "deserialize_l1")]
    pub l1: TierKeep,
    #[serde(default = "default_l2", deserialize_with = "deserialize_l2")]
    pub l2: TierKeep,
    #[serde(default = "default_l3", deserialize_with = "deserialize_l3")]
    pub l3: ToggledTierKeep,
    #[serde(default = "default_l4", deserialize_with = "deserialize_l4")]
    pub l4: ToggledTierKeep,
    pub snapshot_json_keep_minutes: i64,
    pub detail_interval_sec: i64,
    pub archive: ArchiveSettings,
    pub disk_check: DiskCheckSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TierKeep {
    pub keep_days: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ToggledTierKeep {
    pub enabled: bool,
    pub keep_days: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ArchiveSettings {
    pub queryable: bool,
    pub cold: bool,
    pub cold_after_months: i64,
    pub directory: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiskCheckSettings {
    pub interval_minutes: i64,
    pub min_free_bytes: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiskPressureState {
    pub active: bool,
    pub since_ms: Option<i64>,
    pub free_bytes: i64,
    pub min_free_bytes: i64,
}

impl Default for RetentionLadder {
    fn default() -> Self {
        Self {
            l1: default_l1(),
            l2: default_l2(),
            l3: default_l3(),
            l4: default_l4(),
            snapshot_json_keep_minutes: 60,
            detail_interval_sec: 60,
            archive: ArchiveSettings::default(),
            disk_check: DiskCheckSettings::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialTierKeep {
    #[serde(default, deserialize_with = "deserialize_present")]
    keep_days: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialToggledTierKeep {
    #[serde(default, deserialize_with = "deserialize_present")]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_present")]
    keep_days: Option<i64>,
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn default_l1() -> TierKeep {
    TierKeep { keep_days: 3 }
}

fn default_l2() -> TierKeep {
    TierKeep { keep_days: 30 }
}

fn default_l3() -> ToggledTierKeep {
    ToggledTierKeep {
        enabled: true,
        keep_days: 90,
    }
}

fn default_l4() -> ToggledTierKeep {
    ToggledTierKeep {
        enabled: true,
        keep_days: 730,
    }
}

fn deserialize_l1<'de, D>(deserializer: D) -> Result<TierKeep, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_tier_keep(deserializer, default_l1())
}

fn deserialize_l2<'de, D>(deserializer: D) -> Result<TierKeep, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_tier_keep(deserializer, default_l2())
}

fn deserialize_tier_keep<'de, D>(deserializer: D, default: TierKeep) -> Result<TierKeep, D::Error>
where
    D: Deserializer<'de>,
{
    let partial = PartialTierKeep::deserialize(deserializer)?;
    Ok(TierKeep {
        keep_days: partial.keep_days.unwrap_or(default.keep_days),
    })
}

fn deserialize_l3<'de, D>(deserializer: D) -> Result<ToggledTierKeep, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_toggled_tier_keep(deserializer, default_l3())
}

fn deserialize_l4<'de, D>(deserializer: D) -> Result<ToggledTierKeep, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_toggled_tier_keep(deserializer, default_l4())
}

fn deserialize_toggled_tier_keep<'de, D>(
    deserializer: D,
    default: ToggledTierKeep,
) -> Result<ToggledTierKeep, D::Error>
where
    D: Deserializer<'de>,
{
    let partial = PartialToggledTierKeep::deserialize(deserializer)?;
    Ok(ToggledTierKeep {
        enabled: partial.enabled.unwrap_or(default.enabled),
        keep_days: partial.keep_days.unwrap_or(default.keep_days),
    })
}

impl Default for ArchiveSettings {
    fn default() -> Self {
        Self {
            queryable: false,
            cold: false,
            cold_after_months: 12,
            directory: String::new(),
        }
    }
}

impl Default for DiskCheckSettings {
    fn default() -> Self {
        Self {
            interval_minutes: 60,
            min_free_bytes: 5 * 1024 * 1024 * 1024,
        }
    }
}

impl RetentionLadder {
    pub fn default_for_serde() -> Self {
        Self::default()
    }

    pub fn validate(
        &self,
        disk_pressure: Option<&DiskPressureState>,
        previous: Option<&RetentionLadder>,
    ) -> Result<(), StoreError> {
        validate_range("retentionLadder.l1.keepDays", self.l1.keep_days, 3, 3_650)?;
        validate_range("retentionLadder.l2.keepDays", self.l2.keep_days, 7, 3_650)?;
        validate_range("retentionLadder.l3.keepDays", self.l3.keep_days, 0, 3_650)?;
        validate_range("retentionLadder.l4.keepDays", self.l4.keep_days, 0, 36_500)?;
        validate_range(
            "retentionLadder.snapshotJsonKeepMinutes",
            self.snapshot_json_keep_minutes,
            60,
            1_440,
        )?;
        validate_range(
            "retentionLadder.detailIntervalSec",
            self.detail_interval_sec,
            15,
            3_600,
        )?;
        validate_range(
            "retentionLadder.archive.coldAfterMonths",
            self.archive.cold_after_months,
            1,
            120,
        )?;
        validate_range(
            "retentionLadder.diskCheck.intervalMinutes",
            self.disk_check.interval_minutes,
            5,
            1_440,
        )?;
        if self.disk_check.min_free_bytes < MIN_FREE_BYTES {
            return Err(StoreError::Validation(format!(
                "retentionLadder.diskCheck.minFreeBytes must be at least {MIN_FREE_BYTES}; observed {}",
                self.disk_check.min_free_bytes
            )));
        }
        if self.l3.enabled && self.l3.keep_days < self.l2.keep_days {
            return Err(StoreError::Validation(format!(
                "retentionLadder.l3.keepDays must be greater than or equal to retentionLadder.l2.keepDays ({}) when retentionLadder.l3.enabled is true; observed {}",
                self.l2.keep_days, self.l3.keep_days
            )));
        }
        if self.l4.enabled && self.l4.keep_days != 0 {
            let (required_field, required_days) = if self.l3.enabled {
                ("retentionLadder.l3.keepDays", self.l3.keep_days)
            } else {
                ("retentionLadder.l2.keepDays", self.l2.keep_days)
            };
            if self.l4.keep_days < required_days {
                return Err(StoreError::Validation(format!(
                    "retentionLadder.l4.keepDays must be 0 (forever) or greater than or equal to {required_field} ({required_days}) when retentionLadder.l4.enabled is true; observed {}",
                    self.l4.keep_days
                )));
            }
        }
        if self.archive.cold && !self.archive.queryable {
            return Err(StoreError::Validation(
                "retentionLadder.archive.cold requires retentionLadder.archive.queryable=true; observed cold=true, queryable=false"
                    .to_string(),
            ));
        }
        if !self.archive.directory.is_empty() && !Path::new(&self.archive.directory).is_absolute() {
            return Err(StoreError::Validation(format!(
                "retentionLadder.archive.directory must be empty or an absolute path; observed {:?}",
                self.archive.directory
            )));
        }
        if let Some(pressure) = disk_pressure
            && pressure.active
            && previous.is_some_and(|previous| self.grows_from(previous))
        {
            return Err(StoreError::Validation(format!(
                "disk pressure active: free {} < minFreeBytes {}; shrink first or free disk",
                pressure.free_bytes, pressure.min_free_bytes
            )));
        }
        Ok(())
    }

    pub fn from_legacy(retention_hours: i64, rollup_retention_days: i64) -> Self {
        let mut ladder = Self::default();
        ladder.apply_legacy_aliases(retention_hours, rollup_retention_days);
        ladder
    }

    pub fn apply_legacy_aliases(&mut self, retention_hours: i64, rollup_retention_days: i64) {
        self.l1.keep_days = retention_hours.saturating_add(23).div_euclid(24).max(3);
        self.l2.keep_days = rollup_retention_days.max(7);
        if self.l3.enabled {
            self.l3.keep_days = self.l3.keep_days.max(self.l2.keep_days);
        }
        let required_l4_days = if self.l3.enabled {
            self.l3.keep_days
        } else {
            self.l2.keep_days
        };
        if self.l4.enabled && self.l4.keep_days != 0 {
            self.l4.keep_days = self.l4.keep_days.max(required_l4_days);
        }
    }

    pub fn to_ladder_config(&self, poll_interval_ms: i64) -> LadderConfig {
        LadderConfig {
            l1_keep_ms: self.l1.keep_days.saturating_mul(DAY_MS),
            l2_keep_ms: self.l2.keep_days.saturating_mul(DAY_MS),
            l3: self
                .l3
                .enabled
                .then(|| self.l3.keep_days.saturating_mul(DAY_MS)),
            l4: self
                .l4
                .enabled
                .then(|| self.l4.keep_days.saturating_mul(DAY_MS)),
            snapshot_json_keep_ms: self.snapshot_json_keep_minutes.saturating_mul(MINUTE_MS),
            detail_interval_ms: self.detail_interval_sec.saturating_mul(1_000),
            poll_interval_ms,
        }
    }

    pub(crate) fn grows_from(&self, previous: &RetentionLadder) -> bool {
        self.l1.keep_days > previous.l1.keep_days
            || self.l2.keep_days > previous.l2.keep_days
            || enabled_horizon_grew(
                self.l3.enabled,
                self.l3.keep_days,
                previous.l3.enabled,
                previous.l3.keep_days,
                false,
            )
            || enabled_horizon_grew(
                self.l4.enabled,
                self.l4.keep_days,
                previous.l4.enabled,
                previous.l4.keep_days,
                true,
            )
            || self.snapshot_json_keep_minutes > previous.snapshot_json_keep_minutes
            || (self.archive.queryable && !previous.archive.queryable)
            || (self.archive.cold && !previous.archive.cold)
    }
}

fn validate_range(field: &str, value: i64, min: i64, max: i64) -> Result<(), StoreError> {
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(StoreError::Validation(format!(
        "{field} must be between {min} and {max}; observed {value}"
    )))
}

fn enabled_horizon_grew(
    enabled: bool,
    keep_days: i64,
    previous_enabled: bool,
    previous_keep_days: i64,
    zero_is_forever: bool,
) -> bool {
    if !enabled {
        return false;
    }
    if !previous_enabled {
        return true;
    }
    if zero_is_forever {
        return keep_days == 0 && previous_keep_days != 0
            || keep_days != 0 && previous_keep_days != 0 && keep_days > previous_keep_days;
    }
    keep_days > previous_keep_days
}
