use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{StoreError, validate_one_of, validate_range};

pub const OTEL_INTERVAL_SEC_RANGE: (i64, i64) = (5, 3_600);
pub const SECRET_SHAPED_KEY_WORDS: [&str; 9] = [
    "secret",
    "token",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "authorization",
    "bearer",
    "credential",
];

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
    pub disabled_metrics: Vec<String>,
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
            disabled_metrics: Vec::new(),
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
                "otel.endpoint must be an http:// or https:// URL with a host and without credentials"
                    .to_string(),
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
        if matches!(
            self.headers_env_var.as_str(),
            "OTEL_EXPORTER_OTLP_HEADERS" | "OTEL_EXPORTER_OTLP_METRICS_HEADERS"
        ) {
            return Err(StoreError::Validation(
                "otel.headersEnvVar must not be OTEL_EXPORTER_OTLP_HEADERS or OTEL_EXPORTER_OTLP_METRICS_HEADERS; tinytop reads headers only from its own variable"
                    .to_string(),
            ));
        }
        let service_name_len = self.service_name.chars().count();
        if !(1..=128).contains(&service_name_len) || self.service_name.chars().any(char::is_control)
        {
            return Err(StoreError::Validation(
                "otel.serviceName must be 1–128 characters without control characters".to_string(),
            ));
        }
        if self.disabled_metrics.len() > 64 {
            return Err(StoreError::Validation(
                "otel.disabledMetrics accepts at most 64 entries".to_string(),
            ));
        }
        let mut disabled_metrics = HashSet::with_capacity(self.disabled_metrics.len());
        for name in &self.disabled_metrics {
            if !valid_disabled_metric_name(name) {
                return Err(StoreError::Validation(
                    "otel.disabledMetrics entries must match ^[a-z][a-z0-9._]*$ and be at most 128 characters"
                        .to_string(),
                ));
            }
            if !disabled_metrics.insert(name.as_str()) {
                return Err(StoreError::Validation(format!(
                    "otel.disabledMetrics must not contain duplicate entry {name:?}"
                )));
            }
        }
        if self.resource_attributes.len() > 32
            || self.resource_attributes.iter().any(|(key, value)| {
                !valid_resource_attribute_key(key)
                    || value.chars().count() > 256
                    || value.chars().any(char::is_control)
            })
        {
            return Err(StoreError::Validation(
                "otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters".to_string(),
            ));
        }
        if self
            .resource_attributes
            .keys()
            .any(|key| secret_shaped_resource_attribute_key(key))
        {
            return Err(StoreError::Validation(
                "otel.resourceAttributes keys must not be secret-shaped (no segment may be secret, token, password, passwd, apikey, api_key, authorization, bearer or credential)"
                    .to_string(),
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
            if authority.is_empty() || authority.contains('@') {
                return false;
            }
            let host = if let Some(ipv6) = authority.strip_prefix('[') {
                let Some(closing_bracket) = ipv6.find(']') else {
                    return false;
                };
                &ipv6[..closing_bracket]
            } else {
                authority
                    .rsplit_once(':')
                    .map_or(authority, |(host, _port)| host)
            };
            !host.is_empty()
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

fn valid_disabled_metric_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && name.chars().count() <= 128
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_')
        })
}

fn secret_shaped_resource_attribute_key(key: &str) -> bool {
    key.split('.').any(|segment| {
        SECRET_SHAPED_KEY_WORDS.contains(&segment)
            || segment
                .split('_')
                .any(|part| SECRET_SHAPED_KEY_WORDS.contains(&part))
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
                disabled_metrics: Vec::new(),
                resource_attributes: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn default_settings_disable_no_metrics() {
        // Break caught: a fresh or legacy document opts out of metrics by default.
        assert!(OtelSettings::default().disabled_metrics.is_empty());
    }

    #[test]
    fn disabled_metrics_rejects_more_than_64_entries() {
        // Break caught: an unbounded disabled set makes the settings document an
        // unnecessary storage and request-size sink.
        let settings = OtelSettings {
            disabled_metrics: (0..65)
                .map(|index| format!("system.metric.{index}"))
                .collect(),
            ..OtelSettings::default()
        };

        assert_eq!(
            validation_message(&settings),
            "otel.disabledMetrics accepts at most 64 entries"
        );
    }

    #[test]
    fn disabled_metrics_rejects_entries_outside_the_name_shape() {
        // Break caught: malformed names enter persisted fleet configuration.
        let message = "otel.disabledMetrics entries must match ^[a-z][a-z0-9._]*$ and be at most 128 characters";
        for name in [
            String::new(),
            "System.cpu.utilization".to_string(),
            "system.cpu-utilization".to_string(),
            format!("a{}", "b".repeat(128)),
        ] {
            let settings = OtelSettings {
                disabled_metrics: vec![name.clone()],
                ..OtelSettings::default()
            };
            assert_eq!(validation_message(&settings), message, "{name:?}");
        }
    }

    #[test]
    fn disabled_metrics_rejects_duplicates_and_names_the_repeated_value() {
        // Break caught: duplicate state makes a set-shaped setting ambiguous.
        let repeated = "system.cpu.utilization";
        let settings = OtelSettings {
            disabled_metrics: vec![repeated.to_string(), repeated.to_string()],
            ..OtelSettings::default()
        };

        assert_eq!(
            validation_message(&settings),
            format!("otel.disabledMetrics must not contain duplicate entry {repeated:?}")
        );
    }

    #[test]
    fn unknown_metric_name_is_accepted_and_preserved() {
        // Break caught: an older tinytop rejects or drops a newer release's choice.
        let unknown = "system.future.metric";
        let mut settings = DashboardSettings::default();
        settings.otel.disabled_metrics = vec![unknown.to_string()];
        settings
            .validate()
            .expect("a well-formed unknown name is valid");

        let before = serde_json::to_vec(&settings).expect("settings should serialize");
        let document = serde_json::from_slice(&before).expect("settings JSON should decode");
        let round_tripped = DashboardSettings::from_document(document, None)
            .expect("settings document should round-trip");
        let after = serde_json::to_vec(&round_tripped).expect("settings should reserialize");

        assert_eq!(round_tripped.otel.disabled_metrics, [unknown]);
        assert_eq!(
            after, before,
            "the unknown metric must survive byte-identically"
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
                "otel.endpoint must be an http:// or https:// URL with a host and without credentials",
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
            "otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters",
        ));

        let mut bad_key = OtelSettings::default();
        bad_key
            .resource_attributes
            .insert("Bad-Key".to_string(), "value".to_string());
        cases.push((
            bad_key,
            "otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters",
        ));

        for (settings, expected) in cases {
            assert_eq!(validation_message(&settings), expected);
        }
    }

    #[test]
    fn endpoint_validation_requires_a_host_and_refuses_credentials() {
        // Break caught: a missing host or URL userinfo reaches Reqwest, where
        // userinfo becomes an Authorization header and leaks through settings.
        let message =
            "otel.endpoint must be an http:// or https:// URL with a host and without credentials";
        for endpoint in [
            "http://:4318/v1/metrics",
            "https://user:sekrit@collector/v1/metrics",
            "https://@collector/v1/metrics",
        ] {
            let settings = OtelSettings {
                endpoint: endpoint.to_string(),
                ..OtelSettings::default()
            };
            assert_eq!(validation_message(&settings), message, "{endpoint}");
        }
        for endpoint in [
            "http://[::1]:4318/v1/metrics",
            "https://collector.example/v1/metrics",
            "http://collector:4318",
        ] {
            let settings = OtelSettings {
                endpoint: endpoint.to_string(),
                ..OtelSettings::default()
            };
            settings
                .validate()
                .unwrap_or_else(|error| panic!("{endpoint} should be accepted: {error}"));
        }
    }

    #[test]
    fn resource_attribute_keys_refuse_secret_shapes_after_syntax_validation() {
        // Break caught: credential-shaped resource attributes enter settings,
        // exported documents, coverage, and exporter resources.
        let message = "otel.resourceAttributes keys must not be secret-shaped (no segment may be secret, token, password, passwd, apikey, api_key, authorization, bearer or credential)";
        for key in ["auth.token", "api_key", "service.api_key", "my_token"] {
            let mut settings = OtelSettings::default();
            settings
                .resource_attributes
                .insert(key.to_string(), "value".to_string());
            assert_eq!(validation_message(&settings), message, "{key}");
        }

        let mut settings = OtelSettings::default();
        settings.resource_attributes.insert(
            "deployment.environment".to_string(),
            "production".to_string(),
        );
        settings
            .validate()
            .expect("a non-secret resource key should remain valid");
    }

    #[test]
    fn resource_attribute_key_length_accepts_64_and_refuses_65_characters() {
        // Break caught: the documented 64-character boundary is off by one.
        let mut settings = OtelSettings::default();
        settings
            .resource_attributes
            .insert("a".repeat(64), "value".to_string());
        settings
            .validate()
            .expect("a 64-character key should be accepted");

        settings.resource_attributes.clear();
        settings
            .resource_attributes
            .insert("a".repeat(65), "value".to_string());
        assert_eq!(
            validation_message(&settings),
            "otel.resourceAttributes must hold at most 32 entries with keys of at most 64 characters matching ^[a-z][a-z0-9._]*$ and values of at most 256 characters"
        );
    }

    #[test]
    fn headers_env_var_refuses_standard_otlp_header_names() {
        // Break caught: the SDK merges its standard variables beside TinyTop's
        // parser, creating a second source and parser for header values.
        let message = "otel.headersEnvVar must not be OTEL_EXPORTER_OTLP_HEADERS or OTEL_EXPORTER_OTLP_METRICS_HEADERS; tinytop reads headers only from its own variable";
        for name in [
            "OTEL_EXPORTER_OTLP_HEADERS",
            "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
        ] {
            let settings = OtelSettings {
                headers_env_var: name.to_string(),
                ..OtelSettings::default()
            };
            assert_eq!(validation_message(&settings), message, "{name}");
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
