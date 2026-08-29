use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{StoreError, validate_one_of, validate_range};

pub const OTEL_INTERVAL_SEC_RANGE: (i64, i64) = (5, 3_600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelSettings {
    pub enabled: bool,
    pub endpoint: String,
    pub protocol: String,
    pub interval_sec: i64,
    pub headers_env_var: String,
    pub service_name: String,
    #[serde(default)]
    pub resource_attributes: BTreeMap<String, String>,
}

impl Default for OtelSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://127.0.0.1:4318/v1/metrics".to_string(),
            protocol: "http/protobuf".to_string(),
            interval_sec: 60,
            headers_env_var: "TINYTOP_OTEL_HEADERS".to_string(),
            service_name: "tinytop".to_string(),
            resource_attributes: BTreeMap::new(),
        }
    }
}

impl OtelSettings {
    pub fn default_for_serde() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> Result<(), StoreError> {
        validate_one_of("otel.protocol", &self.protocol, &["http/protobuf"])?;
        if !valid_http_endpoint(&self.endpoint) {
            return Err(StoreError::Validation(
                "otel.endpoint must be an http:// or https:// URL with a host".to_string(),
            ));
        }
        validate_range(
            "otel.intervalSec",
            self.interval_sec,
            OTEL_INTERVAL_SEC_RANGE.0,
            OTEL_INTERVAL_SEC_RANGE.1,
        )?;
        if !valid_headers_env_var(&self.headers_env_var) {
            return Err(StoreError::Validation(
                "otel.headersEnvVar must match ^[A-Z][A-Z0-9_]*$".to_string(),
            ));
        }
        let service_name_len = self.service_name.chars().count();
        if !(1..=128).contains(&service_name_len) || self.service_name.chars().any(char::is_control)
        {
            return Err(StoreError::Validation(
                "otel.serviceName must be 1–128 characters without control characters".to_string(),
            ));
        }
        if self.resource_attributes.len() > 32
            || self.resource_attributes.iter().any(|(key, value)| {
                !valid_resource_attribute_key(key)
                    || value.chars().count() > 256
                    || value.chars().any(char::is_control)
            })
        {
            return Err(StoreError::Validation(
                "otel.resourceAttributes must hold at most 32 entries with keys matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters".to_string(),
            ));
        }
        Ok(())
    }
}

fn valid_http_endpoint(endpoint: &str) -> bool {
    if endpoint
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return false;
    }
    endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .map(|value| {
            let authority = value
                .split_once(['/', '?', '#'])
                .map_or(value, |(authority, _)| authority);
            !authority.is_empty()
        })
        .unwrap_or(false)
}

fn valid_headers_env_var(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

fn valid_resource_attribute_key(key: &str) -> bool {
    let mut characters = key.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && key.chars().count() <= 64
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_')
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::{DashboardSettings, StoreError};

    fn validation_message(settings: &OtelSettings) -> String {
        match settings.validate().expect_err("fixture should be refused") {
            StoreError::Validation(message) => message,
            other => panic!("expected validation error, got {other}"),
        }
    }

    #[test]
    fn defaults_match_the_otel_contract() {
        // Break caught: a fresh or legacy settings document silently exports telemetry.
        assert_eq!(
            OtelSettings::default(),
            OtelSettings {
                enabled: false,
                endpoint: "http://127.0.0.1:4318/v1/metrics".to_string(),
                protocol: "http/protobuf".to_string(),
                interval_sec: 60,
                headers_env_var: "TINYTOP_OTEL_HEADERS".to_string(),
                service_name: "tinytop".to_string(),
                resource_attributes: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn validation_refuses_every_invalid_otel_field_with_the_contract_message() {
        // Break caught: malformed exporter settings reach pipeline construction.
        let mut cases: Vec<(OtelSettings, &str)> = Vec::new();

        let value = OtelSettings {
            protocol: "grpc".to_string(),
            ..OtelSettings::default()
        };
        cases.push((value, "otel.protocol must be one of http/protobuf"));

        for endpoint in [
            "collector:4318/v1/metrics",
            "http:///v1/metrics",
            "https://bad host/v1/metrics",
        ] {
            let value = OtelSettings {
                endpoint: endpoint.to_string(),
                ..OtelSettings::default()
            };
            cases.push((
                value,
                "otel.endpoint must be an http:// or https:// URL with a host",
            ));
        }

        for interval in [4, 3_601] {
            let value = OtelSettings {
                interval_sec: interval,
                ..OtelSettings::default()
            };
            cases.push((value, "otel.intervalSec must be between 5 and 3600"));
        }

        for name in ["tinytop_headers", "1TINYTOP_HEADERS"] {
            let value = OtelSettings {
                headers_env_var: name.to_string(),
                ..OtelSettings::default()
            };
            cases.push((value, "otel.headersEnvVar must match ^[A-Z][A-Z0-9_]*$"));
        }

        for service_name in [String::new(), "x".repeat(129)] {
            let value = OtelSettings {
                service_name,
                ..OtelSettings::default()
            };
            cases.push((
                value,
                "otel.serviceName must be 1–128 characters without control characters",
            ));
        }

        let too_many = OtelSettings {
            resource_attributes: (0..33)
                .map(|index| (format!("key.{index}"), "value".to_string()))
                .collect(),
            ..OtelSettings::default()
        };
        cases.push((
            too_many,
            "otel.resourceAttributes must hold at most 32 entries with keys matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters",
        ));

        let mut bad_key = OtelSettings::default();
        bad_key
            .resource_attributes
            .insert("Bad-Key".to_string(), "value".to_string());
        cases.push((
            bad_key,
            "otel.resourceAttributes must hold at most 32 entries with keys matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters",
        ));

        for (settings, expected) in cases {
            assert_eq!(validation_message(&settings), expected);
        }
    }

    #[test]
    fn document_without_otel_keeps_persisted_or_uses_default() {
        // Break caught: importing a 0.4.1 document disables a configured exporter.
        let mut persisted = DashboardSettings::default();
        persisted.otel.enabled = true;
        persisted.otel.endpoint = "https://collector.example/v1/metrics".to_string();
        let mut document = serde_json::to_value(DashboardSettings::default()).unwrap();
        document.as_object_mut().unwrap().remove("otel");

        let with_persisted = DashboardSettings::from_document(document.clone(), Some(&persisted))
            .expect("legacy settings should decode");
        let without_persisted = DashboardSettings::from_document(document, None)
            .expect("legacy defaults should decode");

        assert_eq!(with_persisted.otel, persisted.otel);
        assert_eq!(without_persisted.otel, OtelSettings::default());
    }

    #[test]
    fn changed_keys_reports_otel_once() {
        // Break caught: OTel changes do not wake the daemon loop through the marker contract.
        let previous = DashboardSettings::default();
        let mut next = previous.clone();
        next.otel.enabled = true;
        next.otel.resource_attributes =
            BTreeMap::from([("deployment.environment".to_string(), "test".to_string())]);

        assert_eq!(DashboardSettings::changed_keys(&previous, &next), ["otel"]);
        assert_eq!(
            json!(DashboardSettings::changed_keys(&previous, &next)),
            json!(["otel"])
        );
    }
}
