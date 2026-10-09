#![cfg(all(feature = "linux-collector", target_os = "linux"))]

//! Per-process CPU, measured on the real collector against a child process
//! that is known to be busy.
//!
//! Through 0.15.0 the collector reported 0.0 % for almost every process (see the
//! comment on `refresh_system` in `src/linux.rs`). Nothing caught it, because
//! every other test either feeds the snapshot builder a fixture or checks the
//! shape of a live sample, never a live CPU figure against a known load.
//!
//! The child is a single-threaded shell loop, so in the documented unit (a
//! percentage of ONE core, `docs/guides/API.md`) it reads close to 100 on an
//! idle host and lower on a loaded one. The bounds are deliberately wide: the
//! floor only has to be far from zero and from a whole-machine percentage
//! (100 / 28 cores = 3.6 on the development host), and the ceiling only has to
//! be far from the several-hundred-percent readings the same defect produced
//! under load.

use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use tinytop_collectors::{Collector, CollectorConfig, linux::LinuxCollector};
use tinytop_types::SystemSnapshot;

/// A single-threaded busy loop must read at least this much of one core.
const BUSY_FLOOR_PERCENT: f64 = 10.0;
/// A single-threaded process cannot use more than one core; the margin covers
/// the two reads (process times, machine times) not being simultaneous.
const ONE_CORE_CEILING_PERCENT: f64 = 150.0;
/// The same margin for the sum of a sample's CPU list against the machine.
const MACHINE_CEILING_FACTOR: f64 = 1.5;
/// The widest setting, so a busy process is in the CPU list on a busy host.
const WIDEST_PROCESS_COUNT: usize = 50;

/// A child that burns one core until it is dropped.
struct BusyChild(Child);

impl BusyChild {
    fn spawn() -> Self {
        let child = Command::new("sh")
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn a busy shell loop");
        // Let it accumulate CPU time before the first read: a process whose
        // counters are still zero has no previous value to subtract from.
        thread::sleep(Duration::from_millis(300));
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for BusyChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn widest_collector() -> LinuxCollector {
    let mut collector = LinuxCollector::with_clock(Instant::now);
    collector.configure(CollectorConfig {
        top_process_count: WIDEST_PROCESS_COUNT,
        ..CollectorConfig::default()
    });
    collector
}

/// The CPU figure of `pid` in the sample's CPU list; `None` when it is not in
/// that list at all, which is itself a wrong answer for a busy process.
fn cpu_list_reading(snapshot: &SystemSnapshot, pid: u32) -> Option<f64> {
    snapshot
        .processes
        .iter()
        .find(|process| process.pid == pid && process.cpu_rank.is_some())
        .map(|process| process.cpu_percent)
}

fn assert_busy_reading(reading: Option<f64>, context: &str) {
    let Some(percent) = reading else {
        panic!("{context}: the busy child is not in the top {WIDEST_PROCESS_COUNT} by CPU");
    };
    assert!(
        (BUSY_FLOOR_PERCENT..=ONE_CORE_CEILING_PERCENT).contains(&percent),
        "{context}: the busy child reads {percent} %, expected {BUSY_FLOOR_PERCENT}..={ONE_CORE_CEILING_PERCENT} % of one core"
    );
}

#[test]
fn a_busy_child_reads_as_a_share_of_one_core_on_every_tick() {
    // Break caught: per-process CPU divided by the machine's CPU time since
    // boot instead of since the previous tick, which reads 0.0 for everything.
    let child = BusyChild::spawn();
    let mut collector = widest_collector();
    collector.collect().expect("first collection");

    let mut readings = Vec::new();
    for tick in 1..=4 {
        thread::sleep(Duration::from_millis(500));
        let snapshot = collector.collect().expect("later collection");
        let reading = cpu_list_reading(&snapshot, child.pid());
        readings.push(reading);
        assert_busy_reading(
            reading,
            &format!("tick {tick} (readings so far {readings:?})"),
        );

        // The whole CPU list cannot add up to more than the machine has. The
        // same defect, when a tick was slow, reported every process several
        // times too high (redis-server at 1353 % on 2026-10-09).
        let listed = snapshot
            .processes
            .iter()
            .filter(|process| process.cpu_rank.is_some())
            .map(|process| process.cpu_percent)
            .sum::<f64>();
        let machine = snapshot.cpu.cores as f64 * 100.0;
        assert!(
            listed <= machine * MACHINE_CEILING_FACTOR,
            "tick {tick}: the CPU list adds up to {listed} % on a machine of {machine} %"
        );
    }
}

#[test]
fn the_first_collection_already_measures_cpu() {
    // A one-shot `collect --json` has no previous tick. Like the host CPU
    // figure, the process figure then comes from two reads a short interval
    // apart inside that one collection, not from a zero.
    let child = BusyChild::spawn();
    let mut collector = widest_collector();
    let snapshot = collector.collect().expect("first collection");
    assert_busy_reading(cpu_list_reading(&snapshot, child.pid()), "first collection");
}

#[test]
fn a_collection_straight_after_another_does_not_understate_cpu() {
    // Break caught: a collection that follows another within sysinfo's
    // minimum CPU interval (an on-demand `/snapshot/collect`, or the timer
    // catching up after a slow tick) divided a few milliseconds of process
    // time by the whole previous interval of machine time.
    let child = BusyChild::spawn();
    let mut collector = widest_collector();
    collector.collect().expect("first collection");
    // A long interval, so that an understated figure is far under the floor:
    // without the guard this read 8.2 % after one second and about half that
    // after two (measured 2026-10-09, debug build, load average 12).
    thread::sleep(Duration::from_secs(2));
    let on_time = collector.collect().expect("collection on time");
    let straight_after = collector.collect().expect("collection straight after");

    assert_busy_reading(cpu_list_reading(&on_time, child.pid()), "on time");
    assert_busy_reading(
        cpu_list_reading(&straight_after, child.pid()),
        "straight after",
    );
}
