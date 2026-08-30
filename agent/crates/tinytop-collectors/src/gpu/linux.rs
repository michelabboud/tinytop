use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::{GpuAdapter, GpuBackend, GpuSample, GpuScanStats, GpuVendor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusySource {
    Unknown,
    Sysfs,
    Fdinfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ClientKey {
    Id { pdev: String, id: String },
    Fd { pdev: String, pid: u32, fd: String },
}

#[derive(Debug, Clone)]
struct EngineReading {
    ns: u64,
    capacity: u64,
}

#[derive(Debug, Clone)]
struct ClientReading {
    adapter_id: String,
    pid: u32,
    engines: HashMap<String, EngineReading>,
}

#[derive(Debug)]
struct ParsedFdinfo {
    driver: String,
    pdev: Option<String>,
    client_id: Option<String>,
    engines: HashMap<String, EngineReading>,
    cycles_only: bool,
}

pub struct LinuxGpuBackend {
    drm_root: PathBuf,
    proc_root: PathBuf,
    nvidia_root: PathBuf,
    clock: Box<dyn FnMut() -> Instant + Send>,
    adapters: Vec<GpuAdapter>,
    device_roots: HashMap<String, PathBuf>,
    busy_sources: HashMap<String, BusySource>,
    previous_clients: HashMap<ClientKey, ClientReading>,
    previous_scan_at: Option<Instant>,
    process_busy: HashMap<u32, f64>,
    last_scan: Option<GpuScanStats>,
    cycles_reported: HashSet<String>,
}

impl LinuxGpuBackend {
    #[doc(hidden)]
    pub fn with_roots(
        drm_root: PathBuf,
        proc_root: PathBuf,
        nvidia_root: PathBuf,
        clock: Box<dyn FnMut() -> Instant + Send>,
    ) -> Self {
        Self {
            drm_root,
            proc_root,
            nvidia_root,
            clock,
            adapters: Vec::new(),
            device_roots: HashMap::new(),
            busy_sources: HashMap::new(),
            previous_clients: HashMap::new(),
            previous_scan_at: None,
            process_busy: HashMap::new(),
            last_scan: None,
            cycles_reported: HashSet::new(),
        }
    }

    pub fn detect_default() -> Option<Box<dyn GpuBackend>> {
        let mut backend = Self::with_roots(
            PathBuf::from("/sys/class/drm"),
            PathBuf::from("/proc"),
            PathBuf::from("/proc/driver/nvidia/gpus"),
            Box::new(Instant::now),
        );
        (!backend.detect().is_empty()).then(|| Box::new(backend) as Box<dyn GpuBackend>)
    }

    fn scan_fdinfo(&mut self, started_at: Instant) -> HashMap<String, Option<f64>> {
        let mut pids_scanned = 0usize;
        let mut pids_denied = 0usize;
        let mut current_clients = HashMap::new();
        let mut pid_entries = read_dir_sorted(&self.proc_root);
        pid_entries.retain(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()))
        });
        let scannable = self
            .adapters
            .iter()
            .filter(|adapter| adapter.driver != "nvidia")
            .collect::<Vec<_>>();

        for pid_entry in pid_entries {
            let Some(pid) = pid_entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let fd_root = pid_entry.path().join("fd");
            let Ok(mut fds) = fs::read_dir(&fd_root).map(|entries| entries.flatten().collect::<Vec<_>>())
            else {
                pids_denied = pids_denied.saturating_add(1);
                continue;
            };
            pids_scanned = pids_scanned.saturating_add(1);
            fds.sort_by_key(|entry| entry.file_name());
            for fd_entry in fds {
                let Ok(target) = fs::read_link(fd_entry.path()) else {
                    continue;
                };
                if !target.to_string_lossy().starts_with("/dev/dri/") {
                    continue;
                }
                let fd = fd_entry.file_name().to_string_lossy().into_owned();
                let Ok(text) = fs::read_to_string(pid_entry.path().join("fdinfo").join(&fd))
                else {
                    continue;
                };
                let Some(parsed) = parse_fdinfo(&text) else {
                    continue;
                };
                let adapter = match parsed.pdev.as_deref() {
                    Some(pdev) => scannable
                        .iter()
                        .find(|adapter| adapter.id == format!("pci-{pdev}"))
                        .copied(),
                    None if scannable.len() == 1 => scannable.first().copied(),
                    None => None,
                };
                let Some(adapter) = adapter else {
                    continue;
                };
                if parsed.cycles_only && self.cycles_reported.insert(parsed.driver.clone()) {
                    eprintln!(
                        "gpu collector info: {} reports drm-cycles engine stats which this version does not read; busy is unavailable",
                        parsed.driver
                    );
                }
                if parsed.engines.is_empty() {
                    continue;
                }
                let pdev = parsed
                    .pdev
                    .unwrap_or_else(|| adapter.id.trim_start_matches("pci-").to_string());
                let key = match parsed.client_id {
                    Some(id) => ClientKey::Id { pdev, id },
                    None => ClientKey::Fd { pdev, pid, fd },
                };
                current_clients.entry(key).or_insert(ClientReading {
                    adapter_id: adapter.id.clone(),
                    pid,
                    engines: parsed.engines,
                });
            }
        }

        let interval = self
            .previous_scan_at
            .and_then(|previous| started_at.checked_duration_since(previous));
        let mut adapter_engines: HashMap<(String, String), (u128, u64)> = HashMap::new();
        let mut pid_engines: HashMap<(u32, String), (u128, u64)> = HashMap::new();
        if interval.is_some_and(|duration| !duration.is_zero()) {
            for (key, current) in &current_clients {
                let Some(previous) = self.previous_clients.get(key) else {
                    continue;
                };
                for (engine, reading) in &current.engines {
                    let Some(previous_reading) = previous.engines.get(engine) else {
                        continue;
                    };
                    let delta = reading.ns.saturating_sub(previous_reading.ns) as u128;
                    add_engine_delta(
                        &mut adapter_engines,
                        (current.adapter_id.clone(), engine.clone()),
                        delta,
                        reading.capacity,
                    );
                    add_engine_delta(
                        &mut pid_engines,
                        (current.pid, engine.clone()),
                        delta,
                        reading.capacity,
                    );
                }
            }
        }

        let mut adapter_busy: HashMap<String, Option<f64>> = self
            .adapters
            .iter()
            .filter(|adapter| adapter.driver != "nvidia")
            .map(|adapter| (adapter.id.clone(), None))
            .collect();
        self.process_busy.clear();
        if let Some(interval) = interval.filter(|duration| !duration.is_zero()) {
            let elapsed_ns = interval.as_nanos();
            for ((adapter_id, _), (delta, capacity)) in adapter_engines {
                let value = utilisation(delta, capacity, elapsed_ns);
                let slot = adapter_busy.entry(adapter_id).or_insert(None);
                *slot = Some(slot.unwrap_or(0.0).max(value));
            }
            for ((pid, _), (delta, capacity)) in pid_engines {
                let value = utilisation(delta, capacity, elapsed_ns);
                self.process_busy
                    .entry(pid)
                    .and_modify(|busy| *busy = busy.max(value))
                    .or_insert(value);
            }
        }

        self.previous_clients = current_clients;
        self.previous_scan_at = Some(started_at);
        let finished_at = (self.clock)();
        self.last_scan = Some(GpuScanStats {
            duration: finished_at
                .checked_duration_since(started_at)
                .unwrap_or(Duration::ZERO),
            pids_scanned,
            pids_denied,
            clients: self.previous_clients.len(),
        });
        adapter_busy
    }
}

impl GpuBackend for LinuxGpuBackend {
    fn detect(&mut self) -> Vec<GpuAdapter> {
        let mut adapters = Vec::new();
        let mut device_roots = HashMap::new();
        for entry in read_dir_sorted(&self.drm_root) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_card_name(&name) {
                continue;
            }
            let device_root = entry.path().join("device");
            let Some(vendor_id) = read_hex_u16(&device_root.join("vendor")) else {
                continue;
            };
            let uevent = fs::read_to_string(device_root.join("uevent")).unwrap_or_default();
            let slot = uevent_value(&uevent, "PCI_SLOT_NAME");
            let id = slot
                .map(|slot| format!("pci-{slot}"))
                .unwrap_or_else(|| name.clone());
            if adapters.iter().any(|adapter: &GpuAdapter| adapter.id == id) {
                continue;
            }
            let driver = uevent_value(&uevent, "DRIVER")
                .unwrap_or("unknown")
                .to_string();
            let vendor_text = read_trimmed(&device_root.join("vendor"))
                .unwrap_or_else(|| format!("0x{vendor_id:04x}"));
            let device_text = read_trimmed(&device_root.join("device"))
                .unwrap_or_else(|| "unknown".to_string());
            let name = read_trimmed(&device_root.join("product_name"))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("{vendor_text}:{device_text}"));
            device_roots.insert(id.clone(), device_root);
            adapters.push(GpuAdapter {
                id,
                vendor: GpuVendor::from_pci_vendor(vendor_id),
                name,
                driver,
            });
        }

        for entry in read_dir_sorted(&self.nvidia_root) {
            let Ok(text) = fs::read_to_string(entry.path().join("information")) else {
                continue;
            };
            let Some(slot) = colon_value(&text, "Bus Location") else {
                continue;
            };
            let id = format!("pci-{slot}");
            if adapters.iter().any(|adapter| adapter.id == id) {
                continue;
            }
            let name = colon_value(&text, "Model")
                .filter(|name| !name.is_empty())
                .unwrap_or("NVIDIA GPU")
                .to_string();
            adapters.push(GpuAdapter {
                id,
                vendor: GpuVendor::Nvidia,
                name,
                driver: "nvidia".to_string(),
            });
        }
        adapters.sort_by(|left, right| left.id.cmp(&right.id));
        self.busy_sources = adapters
            .iter()
            .filter(|adapter| adapter.driver != "nvidia")
            .map(|adapter| (adapter.id.clone(), BusySource::Unknown))
            .collect();
        self.adapters = adapters;
        self.device_roots = device_roots;
        self.previous_clients.clear();
        self.previous_scan_at = None;
        self.process_busy.clear();
        self.last_scan = None;
        self.cycles_reported.clear();
        self.adapters.clone()
    }

    fn sample(&mut self) -> Vec<GpuSample> {
        let started_at = (self.clock)();
        let should_scan = self.adapters.iter().any(|adapter| adapter.driver != "nvidia");
        let fdinfo_busy = if should_scan {
            self.scan_fdinfo(started_at)
        } else {
            HashMap::new()
        };
        if !should_scan {
            self.process_busy.clear();
            self.last_scan = Some(GpuScanStats {
                duration: Duration::ZERO,
                pids_scanned: 0,
                pids_denied: 0,
                clients: 0,
            });
        }

        self.adapters
            .iter()
            .map(|adapter| {
                if adapter.driver == "nvidia" {
                    return empty_sample(&adapter.id);
                }
                let Some(device_root) = self.device_roots.get(&adapter.id) else {
                    return empty_sample(&adapter.id);
                };
                let source = self
                    .busy_sources
                    .entry(adapter.id.clone())
                    .or_insert(BusySource::Unknown);
                let busy_percent = match *source {
                    BusySource::Unknown => {
                        match read_trimmed(&device_root.join("gpu_busy_percent"))
                            .and_then(|value| value.parse::<f64>().ok())
                        {
                            Some(value) => {
                                *source = BusySource::Sysfs;
                                Some(value)
                            }
                            None => {
                                *source = BusySource::Fdinfo;
                                fdinfo_busy.get(&adapter.id).copied().flatten()
                            }
                        }
                    }
                    BusySource::Sysfs => read_trimmed(&device_root.join("gpu_busy_percent"))
                        .and_then(|value| value.parse::<f64>().ok()),
                    BusySource::Fdinfo => fdinfo_busy.get(&adapter.id).copied().flatten(),
                };
                let memory = read_trimmed(&device_root.join("mem_info_vram_used"))
                    .and_then(|value| value.parse::<u64>().ok())
                    .zip(
                        read_trimmed(&device_root.join("mem_info_vram_total"))
                            .and_then(|value| value.parse::<u64>().ok()),
                    );
                GpuSample {
                    adapter_id: adapter.id.clone(),
                    busy_percent,
                    memory_used_bytes: memory.map(|(used, _)| used),
                    memory_total_bytes: memory.map(|(_, total)| total),
                    temperature_c: first_temperature(device_root),
                }
            })
            .collect()
    }

    fn process_busy(&mut self) -> HashMap<u32, f64> {
        self.process_busy.clone()
    }

    fn last_scan(&self) -> Option<GpuScanStats> {
        self.last_scan.clone()
    }
}

fn empty_sample(adapter_id: &str) -> GpuSample {
    GpuSample {
        adapter_id: adapter_id.to_string(),
        busy_percent: None,
        memory_used_bytes: None,
        memory_total_bytes: None,
        temperature_c: None,
    }
}

fn add_engine_delta<K: Eq + std::hash::Hash>(
    target: &mut HashMap<K, (u128, u64)>,
    key: K,
    delta: u128,
    capacity: u64,
) {
    target
        .entry(key)
        .and_modify(|(sum, known_capacity)| {
            *sum = sum.saturating_add(delta);
            *known_capacity = (*known_capacity).max(capacity.max(1));
        })
        .or_insert((delta, capacity.max(1)));
}

fn utilisation(delta: u128, capacity: u64, elapsed_ns: u128) -> f64 {
    let denominator = elapsed_ns.saturating_mul(u128::from(capacity.max(1)));
    if denominator == 0 {
        return 0.0;
    }
    ((delta as f64 / denominator as f64) * 100.0).clamp(0.0, 100.0)
}

fn parse_fdinfo(text: &str) -> Option<ParsedFdinfo> {
    let mut driver = None;
    let mut pdev = None;
    let mut client_id = None;
    let mut engine_ns = HashMap::new();
    let mut capacities = HashMap::new();
    let mut cycles_only = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "drm-driver" => driver = (!value.is_empty()).then(|| value.to_string()),
            "drm-pdev" => pdev = (!value.is_empty()).then(|| value.to_string()),
            "drm-client-id" => client_id = (!value.is_empty()).then(|| value.to_string()),
            _ if key.starts_with("drm-engine-capacity-") => {
                if let Ok(capacity) = value.parse::<u64>() {
                    capacities.insert(
                        key.trim_start_matches("drm-engine-capacity-").to_string(),
                        capacity.max(1),
                    );
                }
            }
            _ if key.starts_with("drm-engine-") => {
                if let Some(ns) = value
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    engine_ns.insert(key.trim_start_matches("drm-engine-").to_string(), ns);
                }
            }
            _ if key.starts_with("drm-cycles-") => cycles_only = true,
            _ => {}
        }
    }
    let driver = driver?;
    let engines = engine_ns
        .into_iter()
        .map(|(name, ns)| {
            let capacity = capacities.get(&name).copied().unwrap_or(1);
            (name, EngineReading { ns, capacity })
        })
        .collect();
    Some(ParsedFdinfo {
        driver,
        pdev,
        client_id,
        engines,
        cycles_only,
    })
}

fn is_card_name(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()))
}

fn read_hex_u16(path: &Path) -> Option<u16> {
    let value = read_trimmed(path)?;
    u16::from_str_radix(value.trim_start_matches("0x"), 16).ok()
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
}

fn read_dir_sorted(path: &Path) -> Vec<fs::DirEntry> {
    let mut entries = fs::read_dir(path)
        .map(|entries| entries.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    entries.sort_by_key(|entry| entry.file_name());
    entries
}

fn uevent_value<'a>(text: &'a str, wanted: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key == wanted).then_some(value.trim())
    })
}

fn colon_value<'a>(text: &'a str, wanted: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == wanted).then_some(value.trim())
    })
}

fn first_temperature(device_root: &Path) -> Option<f64> {
    for hwmon in read_dir_sorted(&device_root.join("hwmon")) {
        if let Some(value) = read_trimmed(&hwmon.path().join("temp1_input"))
            .and_then(|value| value.parse::<f64>().ok())
        {
            return Some(value / 1000.0);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::symlink,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use super::LinuxGpuBackend;
    use crate::gpu::GpuBackend;

    struct Fixture {
        root: PathBuf,
        drm: PathBuf,
        proc: PathBuf,
        nvidia: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "tinytop-gpu-{name}-{}",
                std::process::id()
            ));
            let drm = root.join("drm");
            let proc = root.join("proc");
            let nvidia = root.join("nvidia");
            fs::create_dir_all(&drm).expect("create drm fixture root");
            fs::create_dir_all(&proc).expect("create proc fixture root");
            fs::create_dir_all(&nvidia).expect("create nvidia fixture root");
            Self {
                root,
                drm,
                proc,
                nvidia,
            }
        }

        fn card(&self, name: &str, slot: &str, driver: &str, vendor: &str, device: &str) {
            let device_root = self.drm.join(name).join("device");
            fs::create_dir_all(&device_root).expect("create card fixture");
            fs::write(device_root.join("vendor"), format!("{vendor}\n"))
                .expect("write vendor");
            fs::write(device_root.join("device"), format!("{device}\n"))
                .expect("write device");
            fs::write(
                device_root.join("uevent"),
                format!("DRIVER={driver}\nPCI_SLOT_NAME={slot}\n"),
            )
            .expect("write uevent");
        }

        fn client(&self, pid: u32, fd: u32, text: &str) {
            let pid_root = self.proc.join(pid.to_string());
            fs::create_dir_all(pid_root.join("fd")).expect("create fd fixture");
            fs::create_dir_all(pid_root.join("fdinfo")).expect("create fdinfo fixture");
            let fd_path = pid_root.join("fd").join(fd.to_string());
            if fs::symlink_metadata(&fd_path).is_err() {
                symlink("/dev/dri/renderD128", fd_path).expect("create drm fd symlink");
            }
            fs::write(pid_root.join("fdinfo").join(fd.to_string()), text)
                .expect("write fdinfo fixture");
        }

        fn write(&self, relative: impl AsRef<Path>, value: &str) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().expect("fixture file parent"))
                .expect("create fixture parent");
            fs::write(path, value).expect("write fixture file");
        }

        fn clocked_backend(&self) -> (LinuxGpuBackend, Arc<Mutex<Duration>>) {
            let base = Instant::now();
            let offset = Arc::new(Mutex::new(Duration::ZERO));
            let clock_offset = Arc::clone(&offset);
            let backend = LinuxGpuBackend::with_roots(
                self.drm.clone(),
                self.proc.clone(),
                self.nvidia.clone(),
                Box::new(move || {
                    base + *clock_offset.lock().expect("fixture clock mutex")
                }),
            );
            (backend, offset)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove GPU fixture tree");
        }
    }

    fn fdinfo(pdev: Option<&str>, client: Option<u64>, lines: &[(&str, u64)]) -> String {
        let mut text = String::from("drm-driver:\tamdgpu\n");
        if let Some(pdev) = pdev {
            text.push_str(&format!("drm-pdev:\t{pdev}\n"));
        }
        if let Some(client) = client {
            text.push_str(&format!("drm-client-id:\t{client}\n"));
        }
        for (engine, ns) in lines {
            text.push_str(&format!("drm-engine-{engine}:\t{ns} ns\n"));
        }
        text.push_str("drm-memory-vram:\t5860 KiB\n");
        text
    }

    fn detected_amd_backend(name: &str) -> (Fixture, LinuxGpuBackend, Arc<Mutex<Duration>>) {
        let fixture = Fixture::new(name);
        fixture.card("card1", "0000:02:00.0", "amdgpu", "0x1002", "0x6810");
        let (mut backend, offset) = fixture.clocked_backend();
        assert_eq!(backend.detect().len(), 1);
        (fixture, backend, offset)
    }

    #[test]
    fn amdgpu_card_with_a_busy_file_reports_sysfs_busy_vram_and_temperature() {
        let (fixture, mut backend, _) = detected_amd_backend("amdgpu-sysfs");
        fixture.write("drm/card1/device/gpu_busy_percent", "37\n");
        fixture.write("drm/card1/device/mem_info_vram_used", "6000640\n");
        fixture.write("drm/card1/device/mem_info_vram_total", "2147483648\n");
        fixture.write("drm/card1/device/hwmon/hwmon0/temp1_input", "44000\n");

        let adapters = backend.detect();
        assert_eq!(adapters[0].id, "pci-0000:02:00.0");
        assert_eq!(adapters[0].vendor.as_str(), "amd");
        assert_eq!(adapters[0].name, "0x1002:0x6810");
        assert_eq!(adapters[0].driver, "amdgpu");
        let sample = &backend.sample()[0];
        assert_eq!(sample.busy_percent, Some(37.0));
        assert_eq!(sample.memory_used_bytes, Some(6_000_640));
        assert_eq!(sample.memory_total_bytes, Some(2_147_483_648));
        assert_eq!(sample.temperature_c, Some(44.0));
    }

    #[test]
    fn an_unreadable_busy_file_is_cached_as_unsupported_until_redetect() {
        let (fixture, mut backend, _) = detected_amd_backend("unsupported-cache");
        fs::create_dir_all(fixture.drm.join("card1/device/gpu_busy_percent"))
            .expect("create unreadable busy fixture");
        assert_eq!(backend.sample()[0].busy_percent, None);
        fs::remove_dir(fixture.drm.join("card1/device/gpu_busy_percent"))
            .expect("replace unreadable busy fixture");
        fixture.write("drm/card1/device/gpu_busy_percent", "50\n");
        assert_eq!(backend.sample()[0].busy_percent, None);
        backend.detect();
        assert_eq!(backend.sample()[0].busy_percent, Some(50.0));
    }

    #[test]
    fn connectors_and_render_nodes_are_not_adapters() {
        let fixture = Fixture::new("card-filter");
        fixture.card("card1", "0000:02:00.0", "amdgpu", "0x1002", "0x6810");
        fixture.card("card1-DP-1", "0000:02:00.0", "amdgpu", "0x1002", "0x6810");
        fixture.card("renderD128", "0000:02:00.0", "amdgpu", "0x1002", "0x6810");
        fixture.write("drm/version", "drm 1.1.0\n");
        let (mut backend, _) = fixture.clocked_backend();
        assert_eq!(backend.detect().len(), 1);
    }

    #[test]
    fn i915_card_without_busy_vram_or_hwmon_reports_none() {
        let fixture = Fixture::new("i915-none");
        fixture.card("card0", "0000:00:02.0", "i915", "0x8086", "0x46d1");
        let (mut backend, _) = fixture.clocked_backend();
        let adapters = backend.detect();
        assert_eq!(adapters[0].id, "pci-0000:00:02.0");
        assert_eq!(adapters[0].vendor.as_str(), "intel");
        assert_eq!(adapters[0].name, "0x8086:0x46d1");
        assert_eq!(adapters[0].driver, "i915");
        let sample = &backend.sample()[0];
        assert_eq!(sample.busy_percent, None);
        assert_eq!(sample.memory_used_bytes, None);
        assert_eq!(sample.memory_total_bytes, None);
        assert_eq!(sample.temperature_c, None);
    }

    #[test]
    fn nvidia_proprietary_is_identity_only() {
        let fixture = Fixture::new("nvidia-identity");
        fixture.write(
            "nvidia/0000:01:00.0/information",
            "Model: NVIDIA RTX 4090\nBus Location: 0000:01:00.0\n",
        );
        fixture.card("card1", "0000:03:00.0", "nvidia", "0x10de", "0x2684");
        fixture.client(
            4242,
            7,
            &fdinfo(Some("0000:03:00.0"), Some(4), &[("gfx", 1_000_000_000)]),
        );
        let (mut backend, _) = fixture.clocked_backend();
        let adapters = backend.detect();
        assert_eq!(adapters.len(), 2);
        assert_eq!(adapters[0].vendor.as_str(), "nvidia");
        assert!(adapters.iter().all(|adapter| adapter.driver == "nvidia"));
        assert!(backend.sample().iter().all(|sample| {
            sample.busy_percent.is_none()
                && sample.memory_used_bytes.is_none()
                && sample.memory_total_bytes.is_none()
                && sample.temperature_c.is_none()
        }));
        assert_eq!(backend.last_scan().expect("scan stats").clients, 0);
    }

    #[test]
    fn an_empty_drm_class_yields_no_backend() {
        let fixture = Fixture::new("empty-drm");
        fixture.write("drm/version", "drm 1.1.0\n");
        let (mut backend, _) = fixture.clocked_backend();
        assert!(backend.detect().is_empty());
    }

    #[test]
    fn fdinfo_engine_deltas_give_busy_per_adapter_and_per_pid() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-delta");
        fixture.client(
            4242,
            7,
            &fdinfo(
                Some("0000:02:00.0"),
                Some(4),
                &[("gfx", 1_000_000_000), ("compute", 0)],
            ),
        );
        assert_eq!(backend.sample()[0].busy_percent, None);
        *offset.lock().expect("clock") = Duration::from_secs(2);
        fixture.client(
            4242,
            7,
            &fdinfo(
                Some("0000:02:00.0"),
                Some(4),
                &[("gfx", 2_000_000_000), ("compute", 0)],
            ),
        );
        assert_eq!(backend.sample()[0].busy_percent, Some(50.0));
        assert_eq!(backend.process_busy().get(&4242), Some(&50.0));
    }

    #[test]
    fn the_same_client_through_two_fds_counts_once() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-dedup");
        let first = fdinfo(Some("0000:02:00.0"), Some(4), &[("gfx", 1_000_000_000)]);
        fixture.client(4242, 7, &first);
        fixture.client(4242, 9, &first);
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(2);
        let second = fdinfo(Some("0000:02:00.0"), Some(4), &[("gfx", 2_000_000_000)]);
        fixture.client(4242, 7, &second);
        fixture.client(4242, 9, &second);
        assert_eq!(backend.sample()[0].busy_percent, Some(50.0));
    }

    #[test]
    fn two_clients_sum_and_the_busiest_engine_wins() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-sum-max");
        fixture.client(1001, 7, &fdinfo(Some("0000:02:00.0"), Some(1), &[("gfx", 0)]));
        fixture.client(1002, 8, &fdinfo(Some("0000:02:00.0"), Some(2), &[("gfx", 0)]));
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(2);
        fixture.client(1001, 7, &fdinfo(Some("0000:02:00.0"), Some(1), &[("gfx", 1_000_000_000), ("compute", 800_000_000)]));
        fixture.client(1002, 8, &fdinfo(Some("0000:02:00.0"), Some(2), &[("gfx", 600_000_000)]));
        assert_eq!(backend.sample()[0].busy_percent, Some(80.0));

        backend.detect();
        fixture.client(1001, 7, &fdinfo(Some("0000:02:00.0"), Some(1), &[("gfx", 0), ("compute", 0)]));
        fixture.client(1002, 8, &fdinfo(Some("0000:02:00.0"), Some(2), &[("gfx", 0)]));
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(4);
        fixture.client(1001, 7, &fdinfo(Some("0000:02:00.0"), Some(1), &[("gfx", 800_000_000), ("compute", 800_000_000)]));
        fixture.client(1002, 8, &fdinfo(Some("0000:02:00.0"), Some(2), &[("gfx", 0)]));
        assert_eq!(backend.sample()[0].busy_percent, Some(40.0));

        backend.detect();
        fixture.client(1001, 7, &fdinfo(Some("0000:02:00.0"), Some(1), &[("gfx", 0)]));
        fixture.client(1002, 8, &fdinfo(Some("0000:02:00.0"), Some(2), &[("gfx", 0)]));
        fixture.client(1003, 9, &fdinfo(Some("0000:02:00.0"), Some(3), &[("gfx", 0)]));
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(6);
        for (pid, fd, client) in [(1001, 7, 1), (1002, 8, 2), (1003, 9, 3)] {
            fixture.client(pid, fd, &fdinfo(Some("0000:02:00.0"), Some(client), &[("gfx", 1_000_000_000)]));
        }
        assert_eq!(backend.sample()[0].busy_percent, Some(100.0));
    }

    #[test]
    fn engine_capacity_divides_the_delta() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-capacity");
        fixture.client(4242, 7, "drm-driver:\tamdgpu\ndrm-pdev:\t0000:02:00.0\ndrm-client-id:\t4\ndrm-engine-video:\t0 ns\ndrm-engine-capacity-video:\t2\n");
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(2);
        fixture.client(4242, 7, "drm-driver:\tamdgpu\ndrm-pdev:\t0000:02:00.0\ndrm-client-id:\t4\ndrm-engine-video:\t2000000000 ns\ndrm-engine-capacity-video:\t2\n");
        assert_eq!(backend.sample()[0].busy_percent, Some(50.0));
    }

    #[test]
    fn a_decreasing_counter_counts_as_zero() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-decrease");
        fixture.client(4242, 7, &fdinfo(Some("0000:02:00.0"), Some(4), &[("gfx", 2_000_000_000)]));
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(2);
        fixture.client(4242, 7, &fdinfo(Some("0000:02:00.0"), Some(4), &[("gfx", 1_000_000_000)]));
        assert_eq!(backend.sample()[0].busy_percent, Some(0.0));
    }

    #[test]
    fn an_unreadable_pid_is_skipped_and_counted() {
        let (fixture, mut backend, _) = detected_amd_backend("fdinfo-denied");
        fs::create_dir_all(fixture.proc.join("4242")).expect("create unreadable pid fixture");
        backend.sample();
        assert_eq!(backend.last_scan().expect("scan stats").pids_denied, 1);
    }

    #[test]
    fn a_client_without_pdev_belongs_to_the_only_adapter() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-no-pdev");
        fixture.client(4242, 7, &fdinfo(None, Some(4), &[("gfx", 0)]));
        backend.sample();
        *offset.lock().expect("clock") = Duration::from_secs(2);
        fixture.client(4242, 7, &fdinfo(None, Some(4), &[("gfx", 1_000_000_000)]));
        assert_eq!(backend.sample()[0].busy_percent, Some(50.0));
    }

    #[test]
    fn xe_cycles_stats_yield_none() {
        let (fixture, mut backend, offset) = detected_amd_backend("fdinfo-cycles");
        let cycles = "drm-driver:\txe\ndrm-pdev:\t0000:02:00.0\ndrm-client-id:\t4\ndrm-cycles-rcs:\t100\ndrm-total-cycles-rcs:\t200\n";
        fixture.client(4242, 7, cycles);
        assert_eq!(backend.sample()[0].busy_percent, None);
        *offset.lock().expect("clock") = Duration::from_secs(2);
        fixture.client(4242, 7, cycles);
        assert_eq!(backend.sample()[0].busy_percent, None);
    }
}
