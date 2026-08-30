use std::{collections::HashMap, time::Duration};

use tinytop_types::{GpuSnapshot, SystemSnapshot};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuVendor {
    Amd,
    Nvidia,
    Intel,
    Apple,
    Microsoft,
    Other(u16),
}

impl GpuVendor {
    pub fn from_pci_vendor(id: u16) -> Self {
        match id {
            0x1002 => Self::Amd,
            0x10de => Self::Nvidia,
            0x8086 => Self::Intel,
            0x106b => Self::Apple,
            0x1414 => Self::Microsoft,
            other => Self::Other(other),
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Amd => "amd".to_string(),
            Self::Nvidia => "nvidia".to_string(),
            Self::Intel => "intel".to_string(),
            Self::Apple => "apple".to_string(),
            Self::Microsoft => "microsoft".to_string(),
            Self::Other(id) => format!("0x{id:04x}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapter {
    pub id: String,
    pub vendor: GpuVendor,
    pub name: String,
    pub driver: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GpuSample {
    pub adapter_id: String,
    pub busy_percent: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub temperature_c: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuScanStats {
    pub duration: Duration,
    pub pids_scanned: usize,
    pub pids_denied: usize,
    pub clients: usize,
}

pub trait GpuBackend: Send {
    fn detect(&mut self) -> Vec<GpuAdapter>;

    /// Sample adapter telemetry and compute the per-process values for this tick.
    /// Callers must invoke this before [`GpuBackend::process_busy`].
    fn sample(&mut self) -> Vec<GpuSample>;

    /// Return the per-process busy percentages computed by the last [`Self::sample`].
    fn process_busy(&mut self) -> HashMap<u32, f64>;

    fn last_scan(&self) -> Option<GpuScanStats>;
}

pub fn detect_backend() -> Option<Box<dyn GpuBackend>> {
    #[cfg(target_os = "linux")]
    {
        return linux::LinuxGpuBackend::detect_default();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::detect_backend();
    }
    #[cfg(target_os = "macos")]
    {
        return macos::detect_backend();
    }
    #[allow(unreachable_code)]
    None
}

pub fn attach_gpu(
    snapshot: &mut SystemSnapshot,
    adapters: &[GpuAdapter],
    samples: &[GpuSample],
    busy: &HashMap<u32, f64>,
) {
    snapshot.gpus = adapters
        .iter()
        .map(|adapter| {
            let sample = samples
                .iter()
                .find(|sample| sample.adapter_id == adapter.id);
            GpuSnapshot {
                id: adapter.id.clone(),
                vendor: adapter.vendor.as_str(),
                name: adapter.name.clone(),
                driver: adapter.driver.clone(),
                busy_percent: sample.and_then(|sample| sample.busy_percent),
                memory_used_bytes: sample.and_then(|sample| sample.memory_used_bytes),
                memory_total_bytes: sample.and_then(|sample| sample.memory_total_bytes),
                temperature_c: sample.and_then(|sample| sample.temperature_c),
            }
        })
        .collect();
    for process in &mut snapshot.processes {
        process.gpu_percent = busy.get(&process.pid).copied();
    }
}
