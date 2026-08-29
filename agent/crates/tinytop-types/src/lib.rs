use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeKind {
    #[serde(rename = "WSL")]
    Wsl,
    Linux,
    Windows,
    #[serde(rename = "macOS")]
    MacOs,
    Unknown,
}

impl RuntimeKind {
    /// Canonical string form of the runtime kind.
    ///
    /// This is the single source of truth shared with anything that persists or
    /// compares runtime kinds as text (e.g. the SQLite store's `runtime_kind`
    /// column). It intentionally mirrors the serde `rename` values above — not
    /// the `Debug` variant names — so a stored value always matches the JSON
    /// contract (`"WSL"`, not `"Wsl"`). The `runtime_kind_as_str_matches_serde`
    /// test in this crate asserts that parity for every variant.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Wsl => "WSL",
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDetection {
    pub kind: RuntimeKind,
    pub confidence: RuntimeConfidence,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySnapshot {
    pub hostname: String,
    pub platform: String,
    pub arch: String,
    pub distro: String,
    pub kernel: String,
    pub runtime: RuntimeDetection,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
    pub total: u64,
    pub idle_total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSnapshot {
    pub usage_percent: f64,
    pub cores: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<CpuTimes>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapSnapshot {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSnapshot {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    pub runnable: u64,
    /// Kernel task total on Linux (`/proc/loadavg`); process count on the
    /// sysinfo-based macOS/Windows collectors, where no thread total exists.
    /// Schema v3 makes this optional.
    pub total_threads: u64,
    pub last_pid: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PressureSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub some: Option<PressureLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<PressureLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureGroup {
    pub cpu: PressureSnapshot,
    pub memory: PressureSnapshot,
    pub io: PressureSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemSnapshot {
    pub filesystem: String,
    #[serde(rename = "type")]
    pub fs_type: String,
    pub size_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub used_percent: f64,
    pub mount: String,
    pub inode_used_percent: Option<f64>,
    pub inode_used: Option<u64>,
    pub inode_total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub command: String,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub rss_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystems_captured_at_ms: Option<i64>,
    pub identity: IdentitySnapshot,
    pub cpu: CpuSnapshot,
    pub memory: MemorySnapshot,
    pub swap: SwapSnapshot,
    pub load: LoadSnapshot,
    pub pressure: PressureGroup,
    pub filesystems: Vec<FilesystemSnapshot>,
    pub processes: Vec<ProcessSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::{RuntimeKind, SystemSnapshot};
    use serde_json::json;

    /// Every `RuntimeKind` variant's canonical `as_str()` must equal its serde
    /// JSON serialization, so persisted text and the JSON contract never diverge
    /// (the M4 bug: `format!("{:?}", ..)` stored `"Wsl"` where JSON says `"WSL"`).
    #[test]
    fn runtime_kind_as_str_matches_serde() {
        for kind in [
            RuntimeKind::Wsl,
            RuntimeKind::Linux,
            RuntimeKind::Windows,
            RuntimeKind::MacOs,
            RuntimeKind::Unknown,
        ] {
            let serialized = serde_json::to_string(&kind).expect("serialize runtime kind");
            let expected = format!("\"{}\"", kind.as_str());
            assert_eq!(
                serialized, expected,
                "as_str() must match serde serialization for {kind:?}"
            );
        }
    }

    #[test]
    fn cpu_times_and_filesystems_captured_at_are_optional_and_additive() {
        let without_optional_fields = json!({
            "timestamp": "2026-08-29T12:00:00Z",
            "identity": {
                "hostname": "fixture-host",
                "platform": "linux",
                "arch": "x86_64",
                "distro": "Fixture Linux",
                "kernel": "6.8.0",
                "runtime": {
                    "kind": "Linux",
                    "confidence": "high",
                    "reason": "fixture"
                },
                "uptimeSeconds": 60
            },
            "cpu": { "usagePercent": 12.5, "cores": 4 },
            "memory": {
                "totalBytes": 100,
                "availableBytes": 40,
                "usedBytes": 60,
                "usedPercent": 60.0
            },
            "swap": {
                "totalBytes": 20,
                "freeBytes": 15,
                "usedBytes": 5,
                "usedPercent": 25.0
            },
            "load": {
                "one": 0.1,
                "five": 0.2,
                "fifteen": 0.3,
                "runnable": 1,
                "totalThreads": 2,
                "lastPid": 3
            },
            "pressure": { "cpu": {}, "memory": {}, "io": {} },
            "filesystems": [],
            "processes": []
        });

        let snapshot: SystemSnapshot = serde_json::from_value(without_optional_fields.clone())
            .expect("optional fields may be absent");
        assert_eq!(snapshot.cpu.times, None);
        assert_eq!(snapshot.filesystems_captured_at_ms, None);
        assert_eq!(
            serde_json::to_value(&snapshot).expect("serialize snapshot"),
            without_optional_fields
        );

        let mut with_optional_fields = without_optional_fields;
        with_optional_fields["cpu"]["times"] = json!({
            "user": 1,
            "nice": 2,
            "system": 3,
            "idle": 4,
            "iowait": 5,
            "irq": 6,
            "softirq": 7,
            "steal": 8,
            "guest": 9,
            "guestNice": 10,
            "total": 55,
            "idleTotal": 9
        });
        with_optional_fields["filesystemsCapturedAtMs"] = json!(1_777_777_777_777_i64);

        let snapshot: SystemSnapshot = serde_json::from_value(with_optional_fields.clone())
            .expect("optional fields may be present");
        assert!(snapshot.cpu.times.is_some());
        assert_eq!(snapshot.filesystems_captured_at_ms, Some(1_777_777_777_777));
        assert_eq!(
            serde_json::to_value(snapshot).expect("serialize additive snapshot"),
            with_optional_fields
        );
    }
}
