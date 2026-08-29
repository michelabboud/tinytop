use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Weak},
    time::Duration,
};

use opentelemetry::{
    KeyValue,
    metrics::{Gauge, MeterProvider as _},
};
use opentelemetry_otlp::{MetricExporter, Protocol, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    error::OTelSdkResult,
    metrics::{
        InstrumentKind, ManualReader, Pipeline, SdkMeterProvider, Temporality,
        data::ResourceMetrics, exporter::PushMetricExporter, reader::MetricReader,
    },
};
use serde::Serialize;
use tinytop_store::{SystemSnapshot, otel_settings::OtelSettings};

const WARN_INTERVAL_MS: i64 = 60_000;
const MAX_ERROR_CHARS: usize = 200;
const FIXED_RESOURCE_KEYS: [&str; 3] = ["service.name", "service.version", "host.name"];
const OTLP_METRICS_HEADERS_ENV: &str = "OTEL_EXPORTER_OTLP_METRICS_HEADERS";
const OTLP_HEADERS_ENV: &str = "OTEL_EXPORTER_OTLP_HEADERS";

/// Operational state exposed by the daemon without retaining exporter secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelStatus {
    pub enabled: bool,
    pub endpoint: String,
    pub interval_sec: i64,
    pub last_success_ms: Option<i64>,
    pub last_failure_ms: Option<i64>,
    pub failures: u64,
    pub last_error: Option<String>,
}

impl OtelStatus {
    pub fn new(enabled: bool, endpoint: impl Into<String>, interval_sec: i64) -> Self {
        Self {
            enabled,
            endpoint: endpoint.into(),
            interval_sec,
            last_success_ms: None,
            last_failure_ms: None,
            failures: 0,
            last_error: None,
        }
    }

    pub fn from_settings(settings: &OtelSettings) -> Self {
        Self::new(
            settings.enabled,
            settings.endpoint.clone(),
            settings.interval_sec,
        )
    }
}

/// Mark an export failure and report whether the caller may emit its warning.
///
/// The warning clock remains separate from [`OtelStatus`] so it is never part of
/// the status or settings JSON contract.
pub fn record_failure(
    status: &mut OtelStatus,
    last_warn_ms: &mut Option<i64>,
    now_ms: i64,
    error: &(impl fmt::Display + ?Sized),
) -> bool {
    status.last_failure_ms = Some(now_ms);
    status.failures = status.failures.saturating_add(1);
    status.last_error = Some(sanitize_error(error));

    let warning_due =
        last_warn_ms.is_none_or(|last| now_ms.saturating_sub(last) >= WARN_INTERVAL_MS);
    if warning_due {
        *last_warn_ms = Some(now_ms);
    }
    warning_due
}

/// Mark an export success, returning whether it recovered from a prior error.
pub fn record_success(status: &mut OtelStatus, now_ms: i64) -> bool {
    let recovered = status.last_failure_ms.is_some_and(|last_failure| {
        status
            .last_success_ms
            .is_none_or(|last_success| last_failure > last_success)
    });
    status.last_success_ms = Some(now_ms);
    recovered
}

pub fn mark_disabled(status: &mut OtelStatus, settings: &OtelSettings) {
    status.enabled = false;
    status.endpoint.clone_from(&settings.endpoint);
    status.interval_sec = settings.interval_sec;
}

pub(crate) fn disable_pipeline(
    pipeline: &mut Option<OtelPipeline>,
    status: &mut OtelStatus,
    settings: &OtelSettings,
    timeout: Duration,
) {
    if let Some(pipeline) = pipeline.take() {
        pipeline.shutdown_best_effort(timeout);
    }
    mark_disabled(status, settings);
}

fn sanitize_error(error: &(impl fmt::Display + ?Sized)) -> String {
    error
        .to_string()
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_ERROR_CHARS)
        .collect()
}

/// Parse the OTLP header environment variable without echoing values in errors.
pub fn parse_otlp_headers(value: Option<&str>) -> Result<HashMap<String, String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(HashMap::new());
    };

    value
        .split(',')
        .enumerate()
        .try_fold(HashMap::new(), |mut headers, (index, entry)| {
            let display_index = index + 1;
            let Some((key, value)) = entry.split_once('=') else {
                return Err(format!("header entry {display_index} is not key=value"));
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(format!("header entry {display_index} is not key=value"));
            }
            let value = value.trim();
            let decoded = if value.contains('%') {
                percent_decode(value).ok_or_else(|| {
                    format!("header entry {display_index} has invalid percent encoding")
                })?
            } else {
                value.to_string()
            };
            headers.insert(key.to_string(), decoded);
            Ok(headers)
        })
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_value)?;
            let low = bytes.get(index + 2).copied().and_then(hex_value)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

// `opentelemetry-otlp` URL-decodes values supplied through `with_headers`.
// F4 already decoded the operator's value, so protect literal percent signs
// from an unintended second decoding pass.
fn headers_for_otlp_builder(headers: HashMap<String, String>) -> HashMap<String, String> {
    headers
        .into_iter()
        .map(|(key, value)| (key, value.replace('%', "%25")))
        .collect()
}

fn preflight_standard_header_env(
    settings: &OtelSettings,
    metrics_headers_present: bool,
    general_headers_present: bool,
) -> Result<(), String> {
    for (name, present) in [
        (OTLP_METRICS_HEADERS_ENV, metrics_headers_present),
        (OTLP_HEADERS_ENV, general_headers_present),
    ] {
        if present && settings.headers_env_var != name {
            return Err(format!(
                "{name} is present but is not the selected otel.headersEnvVar"
            ));
        }
    }
    Ok(())
}

fn preflight_process_header_env(settings: &OtelSettings) -> Result<(), String> {
    preflight_standard_header_env(
        settings,
        std::env::var_os(OTLP_METRICS_HEADERS_ENV).is_some(),
        std::env::var_os(OTLP_HEADERS_ENV).is_some(),
    )
}

#[derive(Debug, Clone)]
struct SharedManualReader(Arc<ManualReader>);

impl SharedManualReader {
    fn new(reader: ManualReader) -> Self {
        Self(Arc::new(reader))
    }
}

impl MetricReader for SharedManualReader {
    fn register_pipeline(&self, pipeline: Weak<Pipeline>) {
        self.0.register_pipeline(pipeline);
    }

    fn collect(&self, resource_metrics: &mut ResourceMetrics) -> OTelSdkResult {
        self.0.collect(resource_metrics)
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.0.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }

    fn temporality(&self, kind: InstrumentKind) -> Temporality {
        self.0.temporality(kind)
    }
}

#[derive(Debug)]
struct Instruments {
    cpu_utilization: Gauge<f64>,
    memory_utilization: Gauge<f64>,
    memory_usage: Gauge<u64>,
    memory_limit: Gauge<u64>,
    paging_utilization: Gauge<f64>,
    load_average_1m: Gauge<f64>,
    load_average_5m: Gauge<f64>,
    load_average_15m: Gauge<f64>,
    filesystem_utilization: Gauge<f64>,
    filesystem_usage: Gauge<u64>,
    load_percent: Gauge<f64>,
    pressure_some: Gauge<f64>,
    pressure_full: Gauge<f64>,
}

impl Instruments {
    fn new(provider: &SdkMeterProvider) -> Self {
        let meter = provider.meter("tinytop-agent");
        Self {
            cpu_utilization: meter
                .f64_gauge("system.cpu.utilization")
                .with_unit("1")
                .build(),
            memory_utilization: meter
                .f64_gauge("system.memory.utilization")
                .with_unit("1")
                .build(),
            memory_usage: meter
                .u64_gauge("system.memory.usage")
                .with_unit("By")
                .build(),
            memory_limit: meter
                .u64_gauge("system.memory.limit")
                .with_unit("By")
                .build(),
            paging_utilization: meter
                .f64_gauge("system.paging.utilization")
                .with_unit("1")
                .build(),
            load_average_1m: meter
                .f64_gauge("system.cpu.load_average.1m")
                .with_unit("{thread}")
                .build(),
            load_average_5m: meter
                .f64_gauge("system.cpu.load_average.5m")
                .with_unit("{thread}")
                .build(),
            load_average_15m: meter
                .f64_gauge("system.cpu.load_average.15m")
                .with_unit("{thread}")
                .build(),
            filesystem_utilization: meter
                .f64_gauge("system.filesystem.utilization")
                .with_unit("1")
                .build(),
            filesystem_usage: meter
                .u64_gauge("system.filesystem.usage")
                .with_unit("By")
                .build(),
            load_percent: meter
                .f64_gauge("tinytop.load.percent")
                .with_unit("1")
                .build(),
            pressure_some: meter
                .f64_gauge("tinytop.pressure.some")
                .with_unit("1")
                .build(),
            pressure_full: meter
                .f64_gauge("tinytop.pressure.full")
                .with_unit("1")
                .build(),
        }
    }
}

/// A caller-driven metrics pipeline. Collection and export happen only when
/// the daemon invokes [`Self::collect_and_export`].
#[derive(Debug)]
pub struct OtelPipeline {
    provider: SdkMeterProvider,
    reader: SharedManualReader,
    exporter: MetricExporter,
    instruments: Instruments,
}

pub fn build_pipeline(
    settings: &OtelSettings,
    headers: HashMap<String, String>,
    hostname: &str,
    timeout: Duration,
) -> Result<OtelPipeline, String> {
    preflight_process_header_env(settings)?;
    let exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(settings.endpoint.clone())
        .with_protocol(Protocol::HttpBinary)
        .with_timeout(timeout)
        .with_headers(headers_for_otlp_builder(headers))
        .with_temporality(Temporality::Delta)
        .build()
        .map_err(|error| sanitize_error(&error))?;
    let reader = SharedManualReader::new(
        ManualReader::builder()
            .with_temporality(Temporality::Delta)
            .build(),
    );
    let resource = fixed_resource(settings, hostname);
    let provider = SdkMeterProvider::builder()
        .with_reader(reader.clone())
        .with_resource(resource)
        .build();
    let instruments = Instruments::new(&provider);

    Ok(OtelPipeline {
        provider,
        reader,
        exporter,
        instruments,
    })
}

fn fixed_resource(settings: &OtelSettings, hostname: &str) -> Resource {
    let mut attributes = settings
        .resource_attributes
        .iter()
        .filter(|(key, _)| !FIXED_RESOURCE_KEYS.contains(&key.as_str()))
        .map(|(key, value)| KeyValue::new(key.clone(), value.clone()))
        .collect::<Vec<_>>();
    attributes.extend([
        KeyValue::new("service.name", settings.service_name.clone()),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("host.name", hostname.to_string()),
    ]);
    Resource::builder_empty()
        .with_attributes(attributes)
        .build()
}

impl OtelPipeline {
    pub fn record_snapshot(&self, snapshot: &SystemSnapshot) {
        self.instruments
            .cpu_utilization
            .record(snapshot.cpu.usage_percent / 100.0, &[]);
        self.instruments
            .memory_utilization
            .record(snapshot.memory.used_percent / 100.0, &[]);
        self.instruments.memory_usage.record(
            snapshot.memory.used_bytes,
            &[KeyValue::new("state", "used")],
        );
        self.instruments
            .memory_limit
            .record(snapshot.memory.total_bytes, &[]);
        let paging_utilization = if snapshot.swap.total_bytes == 0 {
            0.0
        } else {
            snapshot.swap.used_percent / 100.0
        };
        self.instruments
            .paging_utilization
            .record(paging_utilization, &[KeyValue::new("state", "used")]);
        self.instruments
            .load_average_1m
            .record(snapshot.load.one, &[]);
        self.instruments
            .load_average_5m
            .record(snapshot.load.five, &[]);
        self.instruments
            .load_average_15m
            .record(snapshot.load.fifteen, &[]);

        for filesystem in &snapshot.filesystems {
            let dimensions = [
                KeyValue::new("mountpoint", filesystem.mount.clone()),
                KeyValue::new("type", filesystem.fs_type.clone()),
            ];
            self.instruments
                .filesystem_utilization
                .record(filesystem.used_percent / 100.0, &dimensions);
            let mut used_dimensions = dimensions.to_vec();
            used_dimensions.push(KeyValue::new("state", "used"));
            self.instruments
                .filesystem_usage
                .record(filesystem.used_bytes, &used_dimensions);
            let mut free_dimensions = dimensions.to_vec();
            free_dimensions.push(KeyValue::new("state", "free"));
            self.instruments
                .filesystem_usage
                .record(filesystem.available_bytes, &free_dimensions);
        }

        self.instruments
            .load_percent
            .record(tinytop_store::load_percent(snapshot), &[]);
        record_pressure(
            &self.instruments.pressure_some,
            "cpu",
            snapshot.pressure.cpu.some.as_ref().map(|line| line.avg10),
        );
        record_pressure(
            &self.instruments.pressure_some,
            "memory",
            snapshot
                .pressure
                .memory
                .some
                .as_ref()
                .map(|line| line.avg10),
        );
        record_pressure(
            &self.instruments.pressure_some,
            "io",
            snapshot.pressure.io.some.as_ref().map(|line| line.avg10),
        );
        record_pressure(
            &self.instruments.pressure_full,
            "cpu",
            snapshot.pressure.cpu.full.as_ref().map(|line| line.avg10),
        );
        record_pressure(
            &self.instruments.pressure_full,
            "memory",
            snapshot
                .pressure
                .memory
                .full
                .as_ref()
                .map(|line| line.avg10),
        );
        record_pressure(
            &self.instruments.pressure_full,
            "io",
            snapshot.pressure.io.full.as_ref().map(|line| line.avg10),
        );
    }

    pub fn collect(&self) -> Result<ResourceMetrics, String> {
        let mut metrics = ResourceMetrics::default();
        self.reader
            .collect(&mut metrics)
            .map_err(|error| sanitize_error(&error))?;
        Ok(metrics)
    }

    pub async fn collect_and_export(&self) -> Result<(), String> {
        let metrics = self.collect()?;
        self.exporter
            .export(&metrics)
            .await
            .map_err(|error| sanitize_error(&error))
    }

    /// Shut down both halves of the manually-driven pipeline without making
    /// daemon shutdown contingent on telemetry cleanup.
    pub fn shutdown_best_effort(&self, timeout: Duration) {
        let _ = self.provider.shutdown_with_timeout(timeout);
        let _ = self.exporter.shutdown_with_timeout(timeout);
    }
}

fn record_pressure(gauge: &Gauge<f64>, resource: &'static str, value: Option<f64>) {
    if let Some(value) = value {
        gauge.record(value, &[KeyValue::new("resource", resource)]);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
        time::Duration,
    };

    use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
    use prost::Message as _;
    use serde_json::json;
    use tinytop_store::{SystemSnapshot, otel_settings::OtelSettings};

    use super::*;

    fn fixture_snapshot() -> SystemSnapshot {
        serde_json::from_value(json!({
            "timestamp": "2026-08-29T12:00:00Z",
            "identity": {
                "hostname": "fixture-host",
                "platform": "linux",
                "arch": "x86_64",
                "distro": "test",
                "kernel": "test",
                "runtime": { "kind": "Linux", "confidence": "high", "reason": "fixture" },
                "uptimeSeconds": 60
            },
            "cpu": {
                "usagePercent": 25.0,
                "cores": 4,
                "times": {
                    "user": 0, "nice": 0, "system": 0, "idle": 0, "iowait": 0,
                    "irq": 0, "softirq": 0, "steal": 0, "guest": 0, "guestNice": 0,
                    "total": 0, "idleTotal": 0
                }
            },
            "memory": {
                "totalBytes": 1_000, "availableBytes": 400, "usedBytes": 600,
                "usedPercent": 60.0
            },
            "swap": {
                "totalBytes": 0, "freeBytes": 0, "usedBytes": 0, "usedPercent": 99.0
            },
            "load": {
                "one": 2.0, "five": 1.5, "fifteen": 1.0, "runnable": 1,
                "totalThreads": 2, "lastPid": 3
            },
            "pressure": {
                "cpu": {
                    "some": { "avg10": 0.25, "avg60": 0.2, "avg300": 0.1, "total": 10 }
                },
                "memory": {
                    "full": { "avg10": 0.5, "avg60": 0.4, "avg300": 0.3, "total": 20 }
                },
                "io": {}
            },
            "filesystems": [{
                "filesystem": "/dev/root", "type": "ext4", "sizeBytes": 1_000,
                "usedBytes": 700, "availableBytes": 300, "usedPercent": 70.0,
                "mount": "/", "inodeUsedPercent": 10.0, "inodeUsed": 1,
                "inodeTotal": 10
            }],
            "processes": []
        }))
        .expect("fixture should match SystemSnapshot")
    }

    fn metric<'a>(
        request: &'a ExportMetricsServiceRequest,
        name: &str,
    ) -> &'a opentelemetry_proto::tonic::metrics::v1::Metric {
        request.resource_metrics[0].scope_metrics[0]
            .metrics
            .iter()
            .find(|metric| metric.name == name)
            .unwrap_or_else(|| panic!("missing metric {name}"))
    }

    fn attribute_map(
        attributes: &[opentelemetry_proto::tonic::common::v1::KeyValue],
    ) -> BTreeMap<String, String> {
        attributes
            .iter()
            .map(|attribute| {
                let value = attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    .and_then(|value| match value {
                        opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                            value,
                        ) => Some(value.clone()),
                        _ => None,
                    })
                    .expect("fixture attributes should be strings");
                (attribute.key.clone(), value)
            })
            .collect()
    }

    fn gauge_data_points(
        metric: &opentelemetry_proto::tonic::metrics::v1::Metric,
    ) -> &[opentelemetry_proto::tonic::metrics::v1::NumberDataPoint] {
        match metric.data.as_ref().expect("metric should carry data") {
            opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                &gauge.data_points
            }
            other => panic!("expected gauge data, got {other:?}"),
        }
    }

    fn first_f64_value(request: &ExportMetricsServiceRequest, name: &str) -> f64 {
        let point = &gauge_data_points(metric(request, name))[0];
        match point
            .value
            .as_ref()
            .expect("data point should carry a value")
        {
            opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(value) => {
                *value
            }
            other => panic!("expected an f64 gauge value, got {other:?}"),
        }
    }

    fn metric_attribute_maps(
        request: &ExportMetricsServiceRequest,
        name: &str,
    ) -> Vec<BTreeMap<String, String>> {
        let mut maps = gauge_data_points(metric(request, name))
            .iter()
            .map(|point| attribute_map(&point.attributes))
            .collect::<Vec<_>>();
        maps.sort();
        maps
    }

    #[test]
    fn parse_otlp_headers_matches_the_environment_contract() {
        assert!(parse_otlp_headers(None).unwrap().is_empty());
        assert!(parse_otlp_headers(Some("")).unwrap().is_empty());
        assert!(parse_otlp_headers(Some("   ")).unwrap().is_empty());

        assert_eq!(
            parse_otlp_headers(Some("authorization=Bearer%20abc, x-tenant = team-a")).unwrap(),
            HashMap::from([
                ("authorization".to_string(), "Bearer abc".to_string()),
                ("x-tenant".to_string(), "team-a".to_string()),
            ])
        );
        assert_eq!(
            parse_otlp_headers(Some("trace=a=b=c")).unwrap()["trace"],
            "a=b=c"
        );

        let refusal = parse_otlp_headers(Some("authorization=sekrit-value,not-a-pair"))
            .expect_err("entry without equals should be refused");
        assert_eq!(refusal, "header entry 2 is not key=value");
        assert!(!refusal.contains("sekrit-value"));
    }

    #[test]
    fn malformed_percent_encoding_is_refused_without_echoing_the_value() {
        let refusal = parse_otlp_headers(Some("authorization=sekrit%QZ"))
            .expect_err("malformed percent encoding should be refused");

        assert_eq!(refusal, "header entry 1 has invalid percent encoding");
        assert!(!refusal.contains("sekrit"));
    }

    #[test]
    fn decoded_percent_is_preserved_through_the_otlp_builder() {
        let parsed = parse_otlp_headers(Some("x-value=%2520")).unwrap();
        assert_eq!(parsed["x-value"], "%20");

        let prepared = headers_for_otlp_builder(parsed);
        assert_eq!(prepared["x-value"], "%2520");
    }

    #[test]
    fn standard_header_environment_is_fail_closed() {
        let mut settings = OtelSettings {
            headers_env_var: "TINYTOP_HEADERS".to_string(),
            ..OtelSettings::default()
        };
        assert!(preflight_standard_header_env(&settings, false, false).is_ok());

        let refusal = preflight_standard_header_env(&settings, true, false)
            .expect_err("an unselected standard variable must be refused");
        assert_eq!(
            refusal,
            "OTEL_EXPORTER_OTLP_METRICS_HEADERS is present but is not the selected otel.headersEnvVar"
        );

        settings.headers_env_var = "OTEL_EXPORTER_OTLP_METRICS_HEADERS".to_string();
        assert!(preflight_standard_header_env(&settings, true, false).is_ok());
        let refusal = preflight_standard_header_env(&settings, true, true)
            .expect_err("the other standard variable must remain absent");
        assert_eq!(
            refusal,
            "OTEL_EXPORTER_OTLP_HEADERS is present but is not the selected otel.headersEnvVar"
        );

        settings.headers_env_var = "OTEL_EXPORTER_OTLP_HEADERS".to_string();
        assert!(preflight_standard_header_env(&settings, false, true).is_ok());
        let refusal = preflight_standard_header_env(&settings, true, true)
            .expect_err("the other standard variable must remain absent");
        assert_eq!(
            refusal,
            "OTEL_EXPORTER_OTLP_METRICS_HEADERS is present but is not the selected otel.headersEnvVar"
        );
        assert!(!refusal.contains('=') && !refusal.contains("value"));
    }

    #[test]
    fn metric_request_carries_the_spec_names_units_and_resource() {
        let settings = OtelSettings {
            endpoint: "http://127.0.0.1:1/v1/metrics".to_string(),
            service_name: "fixture-service".to_string(),
            resource_attributes: BTreeMap::from([
                ("deployment.environment".to_string(), "test".to_string()),
                ("service.name".to_string(), "must-not-win".to_string()),
            ]),
            ..OtelSettings::default()
        };
        let pipeline = build_pipeline(
            &settings,
            HashMap::new(),
            "fixture-host",
            Duration::from_secs(2),
        )
        .expect("pipeline should build without contacting the endpoint");

        pipeline.record_snapshot(&fixture_snapshot());
        let resource_metrics = pipeline
            .collect()
            .expect("manual collection should succeed");
        let request = ExportMetricsServiceRequest::from(&resource_metrics);

        let mut names = request.resource_metrics[0].scope_metrics[0]
            .metrics
            .iter()
            .map(|metric| metric.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        eprintln!("sorted OTLP metric names: {names:?}");
        assert_eq!(
            names,
            [
                "system.cpu.load_average.15m",
                "system.cpu.load_average.1m",
                "system.cpu.load_average.5m",
                "system.cpu.utilization",
                "system.filesystem.usage",
                "system.filesystem.utilization",
                "system.memory.limit",
                "system.memory.usage",
                "system.memory.utilization",
                "system.paging.utilization",
                "tinytop.load.percent",
                "tinytop.pressure.full",
                "tinytop.pressure.some",
            ]
        );

        let units = request.resource_metrics[0].scope_metrics[0]
            .metrics
            .iter()
            .map(|metric| (metric.name.as_str(), metric.unit.as_str()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            units,
            BTreeMap::from([
                ("system.cpu.load_average.15m", "{thread}"),
                ("system.cpu.load_average.1m", "{thread}"),
                ("system.cpu.load_average.5m", "{thread}"),
                ("system.cpu.utilization", "1"),
                ("system.filesystem.usage", "By"),
                ("system.filesystem.utilization", "1"),
                ("system.memory.limit", "By"),
                ("system.memory.usage", "By"),
                ("system.memory.utilization", "1"),
                ("system.paging.utilization", "1"),
                ("tinytop.load.percent", "1"),
                ("tinytop.pressure.full", "1"),
                ("tinytop.pressure.some", "1"),
            ])
        );
        assert_eq!(first_f64_value(&request, "system.cpu.utilization"), 0.25);
        assert_eq!(first_f64_value(&request, "system.paging.utilization"), 0.0);
        assert_eq!(first_f64_value(&request, "tinytop.load.percent"), 50.0);

        let filesystem_points = gauge_data_points(metric(&request, "system.filesystem.usage"));
        let filesystem_attributes = filesystem_points
            .iter()
            .map(|point| attribute_map(&point.attributes))
            .collect::<Vec<_>>();
        assert_eq!(filesystem_attributes.len(), 2);
        assert!(filesystem_attributes.contains(&BTreeMap::from([
            ("mountpoint".to_string(), "/".to_string()),
            ("state".to_string(), "used".to_string()),
            ("type".to_string(), "ext4".to_string()),
        ])));
        assert!(filesystem_attributes.contains(&BTreeMap::from([
            ("mountpoint".to_string(), "/".to_string()),
            ("state".to_string(), "free".to_string()),
            ("type".to_string(), "ext4".to_string()),
        ])));

        let some_resources = gauge_data_points(metric(&request, "tinytop.pressure.some"))
            .iter()
            .map(|point| attribute_map(&point.attributes)["resource"].clone())
            .collect::<Vec<_>>();
        let full_resources = gauge_data_points(metric(&request, "tinytop.pressure.full"))
            .iter()
            .map(|point| attribute_map(&point.attributes)["resource"].clone())
            .collect::<Vec<_>>();
        assert_eq!(some_resources, ["cpu"]);
        assert_eq!(full_resources, ["memory"]);

        for name in [
            "system.cpu.utilization",
            "system.memory.utilization",
            "system.memory.limit",
            "system.cpu.load_average.1m",
            "system.cpu.load_average.5m",
            "system.cpu.load_average.15m",
            "tinytop.load.percent",
        ] {
            assert_eq!(metric_attribute_maps(&request, name), [BTreeMap::new()]);
        }
        assert_eq!(
            metric_attribute_maps(&request, "system.memory.usage"),
            [BTreeMap::from([("state".to_string(), "used".to_string())])]
        );
        assert_eq!(
            metric_attribute_maps(&request, "system.paging.utilization"),
            [BTreeMap::from([("state".to_string(), "used".to_string())])]
        );
        assert_eq!(
            metric_attribute_maps(&request, "system.filesystem.utilization"),
            [BTreeMap::from([
                ("mountpoint".to_string(), "/".to_string()),
                ("type".to_string(), "ext4".to_string()),
            ])]
        );

        let resource = request.resource_metrics[0]
            .resource
            .as_ref()
            .expect("request should carry a resource");
        let resource_attributes = attribute_map(&resource.attributes);
        assert_eq!(resource_attributes["service.name"], "fixture-service");
        assert_eq!(
            resource_attributes["service.version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(resource_attributes["host.name"], "fixture-host");
        assert_eq!(resource_attributes["deployment.environment"], "test");
        assert_eq!(resource_attributes.len(), 4);
    }

    #[test]
    fn successive_snapshots_export_only_the_latest_attribute_series() {
        let settings = OtelSettings::default();
        let pipeline = build_pipeline(
            &settings,
            HashMap::new(),
            "fixture-host",
            Duration::from_secs(2),
        )
        .expect("pipeline should build");
        let first = fixture_snapshot();
        pipeline.record_snapshot(&first);
        pipeline.collect().expect("first collection should succeed");

        let mut second = first;
        second.cpu.usage_percent = 80.0;
        second.filesystems.clear();
        second.pressure.cpu.some = None;
        second.pressure.memory.full = None;
        pipeline.record_snapshot(&second);
        let second_metrics = pipeline
            .collect()
            .expect("second collection should succeed");
        let request = ExportMetricsServiceRequest::from(&second_metrics);

        assert_eq!(first_f64_value(&request, "system.cpu.utilization"), 0.8);
        for stale_name in [
            "system.filesystem.utilization",
            "system.filesystem.usage",
            "tinytop.pressure.some",
            "tinytop.pressure.full",
        ] {
            let stale_points = request.resource_metrics[0].scope_metrics[0]
                .metrics
                .iter()
                .find(|metric| metric.name == stale_name)
                .map(gauge_data_points)
                .unwrap_or_default();
            assert!(
                stale_points.is_empty(),
                "stale series remained for {stale_name}"
            );
        }
    }

    #[test]
    fn success_recovery_uses_timestamps_and_preserves_last_error() {
        let mut status = OtelStatus::new(true, "endpoint", 60);
        status.last_failure_ms = Some(10);
        status.last_error = Some("preserved diagnostic".to_string());

        assert!(record_success(&mut status, 20));
        assert_eq!(status.last_success_ms, Some(20));
        assert_eq!(status.last_error.as_deref(), Some("preserved diagnostic"));
        assert!(!record_success(&mut status, 30));

        status.last_failure_ms = Some(40);
        assert!(record_success(&mut status, 50));
        assert_eq!(status.last_error.as_deref(), Some("preserved diagnostic"));
        status.last_failure_ms = Some(50);
        assert!(!record_success(&mut status, 60));
    }

    #[test]
    fn warn_is_rate_limited() {
        let mut status = OtelStatus::new(true, "https://collector.example/v1/metrics", 60);
        let mut last_warn_ms = None;

        assert!(record_failure(
            &mut status,
            &mut last_warn_ms,
            1_000,
            "first failure",
        ));
        assert!(!record_failure(
            &mut status,
            &mut last_warn_ms,
            30_000,
            "second failure",
        ));
        assert!(record_failure(
            &mut status,
            &mut last_warn_ms,
            61_000,
            "third failure",
        ));

        assert_eq!(status.failures, 3);
        assert_eq!(status.last_failure_ms, Some(61_000));
        assert_eq!(status.last_error.as_deref(), Some("third failure"));
    }

    #[test]
    fn status_failure_is_sanitized_and_truncated() {
        let unsafe_error = format!("line one\n{}", "é".repeat(250));
        let mut long_status = OtelStatus::new(true, "endpoint", 60);
        let mut separate_warn = None;
        record_failure(&mut long_status, &mut separate_warn, 1_000, &unsafe_error);
        let sanitized = long_status.last_error.expect("failure should be stored");
        assert_eq!(sanitized.chars().count(), 200);
        assert!(!sanitized.contains('\n'));
        assert!(sanitized.is_char_boundary(sanitized.len()));
    }

    #[tokio::test]
    async fn export_failure_increments_the_counter_and_never_stalls_collection() {
        // Break caught: an unreachable receiver blocks native collection or leaks a header value.
        let settings = OtelSettings {
            enabled: true,
            endpoint: "http://127.0.0.1:1/v1/metrics".to_string(),
            ..OtelSettings::default()
        };
        let headers = parse_otlp_headers(Some("authorization=sekrit-value"))
            .expect("fixture header should parse");
        let pipeline = build_pipeline(&settings, headers, "fixture-host", Duration::from_secs(2))
            .expect("pipeline should build without connecting");
        pipeline.record_snapshot(&fixture_snapshot());
        let (_fixture, state) = crate::writer::tests::test_state("otel-failure-collection").await;

        let (failure, collections) = tokio::join!(pipeline.collect_and_export(), async {
            crate::writer::collect_and_store(&state)
                .await
                .expect("first collection should complete");
            crate::writer::collect_and_store(&state)
                .await
                .expect("second collection should complete");
            crate::writer::tests::test_store(&state)
                .stats()
                .await
                .expect("store stats should read")
        });

        let failure = failure.expect_err("port 1 should refuse the export");
        let mut status = OtelStatus::from_settings(&settings);
        let mut last_warn_ms = None;
        assert!(record_failure(
            &mut status,
            &mut last_warn_ms,
            1_000,
            &failure,
        ));
        assert_eq!(status.failures, 1);
        assert!(status.last_error.is_some());
        assert!(
            !status
                .last_error
                .as_deref()
                .unwrap()
                .contains("sekrit-value")
        );
        assert_eq!(collections.sample_count, 2);
    }

    #[test]
    fn disabled_block_tears_the_pipeline_down() {
        // Break caught: disabling a configured exporter leaves a live pipeline or enabled status.
        let enabled = OtelSettings {
            enabled: true,
            endpoint: "http://127.0.0.1:1/v1/metrics".to_string(),
            ..OtelSettings::default()
        };
        let mut pipeline = Some(
            build_pipeline(
                &enabled,
                HashMap::new(),
                "fixture-host",
                Duration::from_millis(10),
            )
            .expect("pipeline should build"),
        );
        let disabled = OtelSettings {
            enabled: false,
            ..enabled.clone()
        };
        let mut status = OtelStatus::from_settings(&enabled);

        disable_pipeline(
            &mut pipeline,
            &mut status,
            &disabled,
            Duration::from_millis(10),
        );

        assert!(pipeline.is_none());
        assert!(!status.enabled);
        assert_eq!(status.failures, 0, "disable must not attempt an export");
    }

    #[tokio::test]
    async fn serve_otel_receiver_decodes_one_request() {
        // Break caught: real OTLP/HTTP output is not protobuf or changes decoded headers/metrics.
        use axum::{
            Router,
            body::Bytes,
            extract::State,
            http::{HeaderMap, StatusCode},
            routing::post,
        };

        type CaptureSender =
            Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<(HeaderMap, Bytes)>>>>;

        async fn capture(
            State(sender): State<CaptureSender>,
            headers: HeaderMap,
            body: Bytes,
        ) -> StatusCode {
            if let Some(sender) = sender.lock().await.take() {
                let _ = sender.send((headers, body));
            }
            StatusCode::OK
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("serve_ tests require sandbox permission to bind loopback");
        let address = listener
            .local_addr()
            .expect("listener should have an address");
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = Arc::new(tokio::sync::Mutex::new(Some(sender)));
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/metrics", post(capture))
                    .with_state(sender),
            )
            .await
        });
        let settings = OtelSettings {
            enabled: true,
            endpoint: format!("http://{address}/v1/metrics"),
            ..OtelSettings::default()
        };
        let headers = parse_otlp_headers(Some("authorization=Bearer%20fixture%2525"))
            .expect("encoded fixture header should parse once");
        let pipeline = build_pipeline(&settings, headers, "fixture-host", Duration::from_secs(2))
            .expect("receiver pipeline should build");
        pipeline.record_snapshot(&fixture_snapshot());

        pipeline
            .collect_and_export()
            .await
            .expect("loopback receiver should accept one export");
        let (headers, body) = receiver.await.expect("receiver should capture one request");
        server.abort();

        assert_eq!(
            headers
                .get("authorization")
                .expect("authorization header should be present"),
            "Bearer fixture%25"
        );
        let request = ExportMetricsServiceRequest::decode(body)
            .expect("request body should be OTLP protobuf");
        let mut names = request.resource_metrics[0].scope_metrics[0]
            .metrics
            .iter()
            .map(|metric| metric.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "system.cpu.load_average.15m",
                "system.cpu.load_average.1m",
                "system.cpu.load_average.5m",
                "system.cpu.utilization",
                "system.filesystem.usage",
                "system.filesystem.utilization",
                "system.memory.limit",
                "system.memory.usage",
                "system.memory.utilization",
                "system.paging.utilization",
                "tinytop.load.percent",
                "tinytop.pressure.full",
                "tinytop.pressure.some",
            ]
        );
    }
}
