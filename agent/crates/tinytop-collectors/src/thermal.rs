use std::{collections::HashMap, ffi::OsString, fs, path::Path};

use tinytop_types::{SensorKind, SensorReading};

pub const CPU_CHIP_ALLOW_LIST: [&str; 2] = ["coretemp", "k10temp"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThermalSensor {
    pub stable_id: String,
    pub chip: String,
    pub label: String,
    pub attr: String,
    hwmon_entry: OsString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThermalScan {
    pub sensors: Vec<ThermalSensor>,
    pub skipped_unnamed_chips: usize,
    pub skipped_chips: usize,
}

pub fn scan(hwmon_root: &Path, extra_chips: &[String]) -> ThermalScan {
    let mut scan = ThermalScan {
        sensors: Vec::new(),
        skipped_unnamed_chips: 0,
        skipped_chips: 0,
    };
    let mut chip_occurrences = HashMap::<String, usize>::new();
    let mut chip_entries = read_dir_sorted(hwmon_root);

    for chip_entry in chip_entries.drain(..) {
        let chip_root = chip_entry.path();
        let Some(chip) = read_trimmed_nonempty(&chip_root.join("name")) else {
            scan.skipped_unnamed_chips += 1;
            continue;
        };
        let occurrence = chip_occurrences.entry(chip.clone()).or_default();
        let chip_index = *occurrence;
        *occurrence += 1;

        if !CPU_CHIP_ALLOW_LIST.contains(&chip.as_str())
            && !extra_chips.iter().any(|extra| extra == &chip)
        {
            scan.skipped_chips += 1;
            continue;
        }

        let mut attributes = read_dir_sorted(&chip_root)
            .into_iter()
            .filter_map(|entry| {
                let file_name = entry.file_name();
                let file_name = file_name.to_str()?;
                let number = file_name
                    .strip_prefix("temp")?
                    .strip_suffix("_input")?
                    .parse::<usize>()
                    .ok()?;
                Some((number, format!("temp{number}")))
            })
            .collect::<Vec<_>>();
        attributes.sort_by_key(|(number, _)| *number);

        for (number, attr) in attributes {
            let label = read_trimmed_nonempty(&chip_root.join(format!("{attr}_label")))
                .unwrap_or_else(|| attr.clone());
            scan.sensors.push(ThermalSensor {
                stable_id: format!("hwmon-{chip}-{chip_index}-temp{number}"),
                chip: chip.clone(),
                label,
                attr,
                hwmon_entry: chip_entry.file_name(),
            });
        }
    }

    scan
}

pub fn read_values(hwmon_root: &Path, sensors: &[ThermalSensor]) -> Vec<SensorReading> {
    sensors
        .iter()
        .filter_map(|sensor| {
            let chip_root = hwmon_root.join(&sensor.hwmon_entry);
            let value = read_millidegrees(&chip_root.join(format!("{}_input", sensor.attr)))?;
            Some(SensorReading {
                stable_id: sensor.stable_id.clone(),
                chip: sensor.chip.clone(),
                kind: SensorKind::Temp,
                label: sensor.label.clone(),
                value,
                max: read_sane_threshold(&chip_root.join(format!("{}_max", sensor.attr))),
                crit: read_sane_threshold(&chip_root.join(format!("{}_crit", sensor.attr))),
            })
        })
        .collect()
}

fn read_sane_threshold(path: &Path) -> Option<f64> {
    read_millidegrees(path).filter(|value| 0.0 < *value && *value <= 200.0)
}

fn read_millidegrees(path: &Path) -> Option<f64> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()
        .map(|value| value as f64 / 1000.0)
}

fn read_trimmed_nonempty(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn read_dir_sorted(path: &Path) -> Vec<fs::DirEntry> {
    let mut entries = fs::read_dir(path)
        .map(|entries| entries.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    entries.sort_by_key(|entry| entry.file_name());
    entries
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{read_values, scan};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("hexe-thermal-{name}-{serial}"));
            fs::create_dir_all(&root).expect("create thermal fixture root");
            Self { root }
        }

        fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().expect("fixture file parent"))
                .expect("create fixture directory");
            fs::write(path, contents).expect("write fixture file");
        }

        fn root(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove thermal fixture root");
        }
    }

    fn write_sensor(
        fixture: &Fixture,
        chip_dir: &str,
        number: usize,
        label: Option<&str>,
        input: &str,
        max: Option<&str>,
        crit: Option<&str>,
    ) {
        fixture.write(&format!("{chip_dir}/temp{number}_input"), input);
        if let Some(label) = label {
            fixture.write(&format!("{chip_dir}/temp{number}_label"), label);
        }
        if let Some(max) = max {
            fixture.write(&format!("{chip_dir}/temp{number}_max"), max);
        }
        if let Some(crit) = crit {
            fixture.write(&format!("{chip_dir}/temp{number}_crit"), crit);
        }
    }

    fn sheep_fixture() -> Fixture {
        let fixture = Fixture::new("sheep");
        fixture.write("hwmon0/name", "nvme\n");
        write_sensor(
            &fixture,
            "hwmon0",
            1,
            Some("Composite\n"),
            "52850\n",
            Some("74850\n"),
            Some("84850\n"),
        );
        write_sensor(
            &fixture,
            "hwmon0",
            2,
            Some("Sensor 1\n"),
            "53850\n",
            Some("65261850\n"),
            None,
        );
        write_sensor(
            &fixture,
            "hwmon0",
            3,
            Some("Sensor 2\n"),
            "52850\n",
            Some("65261850\n"),
            None,
        );
        write_sensor(
            &fixture,
            "hwmon0",
            4,
            Some("Sensor 3\n"),
            "52850\n",
            None,
            None,
        );

        fixture.write("hwmon1/name", "coretemp\n");
        for (number, label, input) in [
            (1, "Package id 0", "54000\n"),
            (2, "Core 0", "54000\n"),
            (3, "Core 1", "53000\n"),
            (4, "Core 2", "53000\n"),
            (5, "Core 3", "53000\n"),
        ] {
            write_sensor(
                &fixture,
                "hwmon1",
                number,
                Some(label),
                input,
                Some("105000\n"),
                Some("105000\n"),
            );
        }
        fixture
    }

    fn trashcan_fixture() -> Fixture {
        let fixture = Fixture::new("trashcan");
        fixture.write("hwmon0/name", "coretemp\n");
        for (number, label, input) in [
            (1, "Package id 0", "55000\n"),
            (2, "Core 0", "55000\n"),
            (3, "Core 1", "49000\n"),
            (4, "Core 2", "47000\n"),
            (5, "Core 3", "45000\n"),
        ] {
            write_sensor(
                &fixture,
                "hwmon0",
                number,
                Some(label),
                input,
                Some("91000\n"),
                Some("105000\n"),
            );
        }
        fixture.write("hwmon1/name", "\n");
        fixture.write("hwmon2/name", "amdgpu\n");
        write_sensor(
            &fixture,
            "hwmon2",
            1,
            Some("edge\n"),
            "46000\n",
            None,
            Some("120000\n"),
        );
        fixture.write("hwmon3/name", "amdgpu\n");
        write_sensor(
            &fixture,
            "hwmon3",
            1,
            Some("edge\n"),
            "47000\n",
            None,
            Some("120000\n"),
        );
        fixture
    }

    #[test]
    fn sheep_coretemp_yields_package_and_four_cores() {
        let fixture = sheep_fixture();
        let scan = scan(fixture.root(), &[]);
        let readings = read_values(fixture.root(), &scan.sensors);

        assert_eq!(scan.sensors.len(), 5);
        assert_eq!(
            scan.sensors
                .iter()
                .map(|sensor| sensor.stable_id.as_str())
                .collect::<Vec<_>>(),
            [
                "hwmon-coretemp-0-temp1",
                "hwmon-coretemp-0-temp2",
                "hwmon-coretemp-0-temp3",
                "hwmon-coretemp-0-temp4",
                "hwmon-coretemp-0-temp5",
            ]
        );
        assert_eq!(
            readings
                .iter()
                .map(|reading| reading.stable_id.as_str())
                .collect::<Vec<_>>(),
            [
                "hwmon-coretemp-0-temp1",
                "hwmon-coretemp-0-temp2",
                "hwmon-coretemp-0-temp3",
                "hwmon-coretemp-0-temp4",
                "hwmon-coretemp-0-temp5",
            ]
        );
        assert_eq!(
            readings
                .iter()
                .map(|reading| reading.label.as_str())
                .collect::<Vec<_>>(),
            ["Package id 0", "Core 0", "Core 1", "Core 2", "Core 3"]
        );
        assert!(readings.iter().all(|reading| reading.chip == "coretemp"));
        assert!(readings.iter().all(|reading| reading.max == Some(105.0)));
        assert!(readings.iter().all(|reading| reading.crit == Some(105.0)));
    }

    #[test]
    fn trashcan_tree_skips_unnamed_chip_and_both_amdgpu() {
        let fixture = trashcan_fixture();
        let scan = scan(fixture.root(), &[]);
        let readings = read_values(fixture.root(), &scan.sensors);

        assert_eq!(scan.sensors.len(), 5);
        assert_eq!(scan.skipped_unnamed_chips, 1);
        assert_eq!(scan.skipped_chips, 2);
        assert_eq!(
            scan.sensors
                .iter()
                .map(|sensor| sensor.stable_id.as_str())
                .collect::<Vec<_>>(),
            [
                "hwmon-coretemp-0-temp1",
                "hwmon-coretemp-0-temp2",
                "hwmon-coretemp-0-temp3",
                "hwmon-coretemp-0-temp4",
                "hwmon-coretemp-0-temp5",
            ]
        );
        assert!(readings.iter().all(|reading| reading.chip == "coretemp"));
        assert!(readings.iter().all(|reading| reading.max == Some(91.0)));
        assert!(readings.iter().all(|reading| reading.crit == Some(105.0)));
        assert!(!readings.iter().any(|reading| reading.chip == "amdgpu"));
    }

    #[test]
    fn bogus_max_is_absent_not_a_number() {
        let fixture = Fixture::new("bogus-max");
        fixture.write("hwmon0/name", "coretemp\n");
        write_sensor(
            &fixture,
            "hwmon0",
            2,
            Some("Core 0\n"),
            "53850\n",
            Some("65261850\n"),
            Some("105000\n"),
        );
        let scan = scan(fixture.root(), &[]);
        let readings = read_values(fixture.root(), &scan.sensors);
        assert_eq!(readings[0].value, 53.85);
        assert_eq!(readings[0].max, None);
    }

    #[test]
    fn missing_max_and_crit_are_absent() {
        let fixture = Fixture::new("missing-thresholds");
        fixture.write("hwmon0/name", "coretemp\n");
        write_sensor(
            &fixture,
            "hwmon0",
            4,
            Some("Sensor 3\n"),
            "52850\n",
            None,
            None,
        );
        let scan = scan(fixture.root(), &[]);
        let readings = read_values(fixture.root(), &scan.sensors);
        assert_eq!(readings[0].max, None);
        assert_eq!(readings[0].crit, None);
    }

    #[test]
    fn label_falls_back_to_attr_name() {
        let fixture = Fixture::new("fallback-label");
        fixture.write("hwmon0/name", "coretemp\n");
        write_sensor(&fixture, "hwmon0", 7, None, "42000\n", None, None);
        let scan = scan(fixture.root(), &[]);
        assert_eq!(scan.sensors[0].label, "temp7");
    }

    #[test]
    fn unreadable_input_omits_the_sensor() {
        let fixture = Fixture::new("unreadable-input");
        fixture.write("hwmon0/name", "coretemp\n");
        write_sensor(
            &fixture,
            "hwmon0",
            1,
            Some("Package id 0"),
            "nonsense\n",
            None,
            None,
        );
        write_sensor(&fixture, "hwmon0", 2, Some("Core 0"), "43000\n", None, None);
        let scan = scan(fixture.root(), &[]);
        let readings = read_values(fixture.root(), &scan.sensors);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].label, "Core 0");
    }

    #[test]
    fn no_hwmon_root_yields_no_sensors() {
        let fixture = Fixture::new("missing-root-parent");
        let scan = scan(&fixture.root().join("does-not-exist"), &[]);
        assert!(scan.sensors.is_empty());
        assert_eq!(scan.skipped_unnamed_chips, 0);
        assert_eq!(scan.skipped_chips, 0);
    }

    #[test]
    fn duplicate_chip_names_get_distinct_ids() {
        let fixture = Fixture::new("duplicate-chips");
        fixture.write("hwmon2/name", "coretemp\n");
        fixture.write("hwmon7/name", "coretemp\n");
        write_sensor(
            &fixture,
            "hwmon2",
            1,
            Some("Package id 0"),
            "40000\n",
            None,
            None,
        );
        write_sensor(
            &fixture,
            "hwmon7",
            1,
            Some("Package id 1"),
            "41000\n",
            None,
            None,
        );
        let scan = scan(fixture.root(), &[]);
        assert_eq!(scan.sensors[0].stable_id, "hwmon-coretemp-0-temp1");
        assert_eq!(scan.sensors[1].stable_id, "hwmon-coretemp-1-temp1");
    }

    #[test]
    fn extra_chips_opts_a_chip_in() {
        let fixture = Fixture::new("extra-chip");
        fixture.write("hwmon0/name", "cpu_thermal\n");
        write_sensor(&fixture, "hwmon0", 1, Some("CPU"), "44000\n", None, None);
        assert!(scan(fixture.root(), &[]).sensors.is_empty());
        let scan = scan(fixture.root(), &["cpu_thermal".to_string()]);
        assert_eq!(scan.sensors.len(), 1);
        assert_eq!(scan.sensors[0].chip, "cpu_thermal");
    }

    #[test]
    fn scan_order_is_deterministic() {
        let fixture = sheep_fixture();
        let first = scan(fixture.root(), &[])
            .sensors
            .into_iter()
            .map(|sensor| sensor.stable_id)
            .collect::<Vec<_>>();
        let second = scan(fixture.root(), &[])
            .sensors
            .into_iter()
            .map(|sensor| sensor.stable_id)
            .collect::<Vec<_>>();
        assert_eq!(first, second);
    }
}
