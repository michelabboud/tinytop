use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalSettings {
    pub enabled: bool,
    #[serde(default)]
    pub extra_chips: Vec<String>,
}

impl Default for ThermalSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            extra_chips: Vec::new(),
        }
    }
}

impl ThermalSettings {
    pub fn default_for_serde() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> Result<(), StoreError> {
        if self.extra_chips.len() > 16 {
            return Err(StoreError::Validation(
                "thermal.extraChips accepts at most 16 chip names".to_string(),
            ));
        }

        let mut seen = HashSet::with_capacity(self.extra_chips.len());
        for chip in &self.extra_chips {
            if chip.is_empty()
                || chip.len() > 32
                || !chip
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(StoreError::Validation(
                    "thermal.extraChips entries must match ^[a-z0-9_]{1,32}$".to_string(),
                ));
            }
            if !seen.insert(chip.as_str()) {
                return Err(StoreError::Validation(format!(
                    "thermal.extraChips contains duplicate chip name \"{chip}\""
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        DashboardSettings, StoreError, settings_transfer::export_document,
        thermal_settings::ThermalSettings,
    };

    fn validation_message(settings: &ThermalSettings) -> String {
        match settings.validate().expect_err("settings should be refused") {
            StoreError::Validation(message) => message,
            other => panic!("expected validation error, got {other}"),
        }
    }

    #[test]
    fn extra_chip_names_refuse_uppercase_and_punctuation() {
        // Break caught: unvalidated names escape the sysfs chip-name grammar.
        let message = "thermal.extraChips entries must match ^[a-z0-9_]{1,32}$";
        for value in ["CoreTemp", "cpu-thermal"] {
            let settings = ThermalSettings {
                enabled: true,
                extra_chips: vec![value.to_string()],
            };
            assert_eq!(validation_message(&settings), message, "{value}");
        }
    }

    #[test]
    fn extra_chip_names_refuse_empty_strings() {
        // Break caught: an empty override can opt in the measured unnamed trashcan chip.
        let settings = ThermalSettings {
            enabled: true,
            extra_chips: vec![String::new()],
        };
        assert_eq!(
            validation_message(&settings),
            "thermal.extraChips entries must match ^[a-z0-9_]{1,32}$"
        );
    }

    #[test]
    fn extra_chip_names_refuse_more_than_sixteen_entries() {
        // Break caught: an unbounded settings document expands the discovery allow-list.
        let settings = ThermalSettings {
            enabled: true,
            extra_chips: (0..17).map(|index| format!("cpu_{index}")).collect(),
        };
        assert_eq!(
            validation_message(&settings),
            "thermal.extraChips accepts at most 16 chip names"
        );
    }

    #[test]
    fn extra_chip_names_refuse_duplicates_and_name_the_value() {
        // Break caught: duplicate overrides survive validation and obscure operator intent.
        let settings = ThermalSettings {
            enabled: true,
            extra_chips: vec!["cpu_thermal".to_string(), "cpu_thermal".to_string()],
        };
        assert_eq!(
            validation_message(&settings),
            "thermal.extraChips contains duplicate chip name \"cpu_thermal\""
        );
    }

    #[test]
    fn absent_thermal_key_keeps_the_persisted_block() {
        // Break caught: importing an older document silently disables configured thermals.
        let mut persisted = DashboardSettings::default();
        persisted.thermal = ThermalSettings {
            enabled: true,
            extra_chips: vec!["cpu_thermal".to_string()],
        };
        let mut document = serde_json::to_value(DashboardSettings::default()).unwrap();
        document.as_object_mut().unwrap().remove("thermal");

        let decoded = DashboardSettings::from_document(document, Some(&persisted))
            .expect("legacy settings should decode");

        assert_eq!(decoded.thermal, persisted.thermal);
    }

    #[test]
    fn changed_keys_names_thermal() {
        // Break caught: a thermal settings change does not wake and reconfigure the collector.
        let previous = DashboardSettings::default();
        let mut next = previous.clone();
        next.thermal.enabled = true;

        assert_eq!(DashboardSettings::changed_keys(&previous, &next), ["thermal"]);
    }

    #[test]
    fn export_document_round_trips_the_full_thermal_block() {
        // Break caught: config export/import drops the opt-in flag or chip overrides.
        let mut settings = DashboardSettings::default();
        settings.thermal = ThermalSettings {
            enabled: true,
            extra_chips: vec!["cpu_thermal".to_string()],
        };
        let document = export_document(&settings, 1_000, "test");
        let value = serde_json::to_value(&document).expect("export should serialize");
        assert_eq!(
            value["settings"]["thermal"],
            json!({"enabled": true, "extraChips": ["cpu_thermal"]})
        );

        let decoded = DashboardSettings::from_document(value["settings"].clone(), None)
            .expect("exported settings should decode");
        assert_eq!(decoded.thermal, settings.thermal);
    }

    #[test]
    fn default_settings_have_thermal_disabled() {
        // Break caught: thermals silently become enabled on upgrade or fresh install.
        let settings = DashboardSettings::default();
        assert!(!settings.thermal.enabled);
        assert!(settings.thermal.extra_chips.is_empty());
    }
}
