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
    pub times: CpuTimes,
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
    use super::RuntimeKind;

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
}
