#[cfg(any(
    all(feature = "macos-collector", target_os = "macos"),
    all(feature = "windows-collector", target_os = "windows"),
))]
mod common;
#[cfg(all(feature = "linux-collector", target_os = "linux"))]
pub mod linux;
#[cfg(all(feature = "macos-collector", target_os = "macos"))]
pub mod macos;
#[cfg(all(feature = "windows-collector", target_os = "windows"))]
pub mod windows;

use std::{fmt, time::Duration};

use tinytop_types::SystemSnapshot;

/// Process totals derived independently of the bounded process snapshot list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessTotals {
    /// Number of processes observed in the full process table.
    pub count: u64,
    /// Highest process identifier observed, or zero when the table is empty.
    pub last_pid: u64,
}

/// Derives the sysinfo collectors' load totals from the full process table
/// before the top-N process list is truncated.
///
/// Sysinfo exposes no thread totals on macOS or Windows, so
/// [`tinytop_types::LoadSnapshot::total_threads`] is the process count there and
/// [`tinytop_types::LoadSnapshot::last_pid`] is the highest PID. Linux instead
/// reads the kernel's task total from `/proc/loadavg`. An honest `Option<u64>`
/// waits for schema v3 (Task 14) because `metric_samples.total_threads` is a
/// `NOT NULL` typed column today.
pub fn process_totals(pids: impl IntoIterator<Item = u64>) -> ProcessTotals {
    pids.into_iter()
        .fold(ProcessTotals::default(), |mut totals, pid| {
            totals.count += 1;
            totals.last_pid = totals.last_pid.max(pid);
            totals
        })
}

pub type CollectorResult<T> = Result<T, CollectorError>;

/// Runtime collector settings. These defaults mirror the store's
/// `topProcessCount` and `retentionLadder.detailIntervalSec` defaults. The
/// daemon configures its collector before the first sample, so they govern only
/// `tinytop-agent collect --json` and direct collector tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectorConfig {
    pub top_process_count: usize,
    pub filesystems_interval: Duration,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            top_process_count: 8,
            filesystems_interval: Duration::from_secs(60),
        }
    }
}

pub trait Collector {
    fn configure(&mut self, config: CollectorConfig);
    fn collect(&mut self) -> CollectorResult<SystemSnapshot>;
}

#[cfg(all(feature = "linux-collector", target_os = "linux"))]
pub type NativeCollector = linux::LinuxCollector;

#[cfg(all(feature = "macos-collector", target_os = "macos"))]
pub type NativeCollector = macos::MacOsCollector;

#[cfg(all(feature = "windows-collector", target_os = "windows"))]
pub type NativeCollector = windows::WindowsCollector;

#[cfg(not(any(
    all(feature = "linux-collector", target_os = "linux"),
    all(feature = "macos-collector", target_os = "macos"),
    all(feature = "windows-collector", target_os = "windows"),
)))]
#[derive(Debug, Default)]
pub struct NativeCollector;

#[cfg(not(any(
    all(feature = "linux-collector", target_os = "linux"),
    all(feature = "macos-collector", target_os = "macos"),
    all(feature = "windows-collector", target_os = "windows"),
)))]
impl Collector for NativeCollector {
    fn configure(&mut self, _config: CollectorConfig) {}

    fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        Self::collect(self)
    }
}

#[cfg(not(any(
    all(feature = "linux-collector", target_os = "linux"),
    all(feature = "macos-collector", target_os = "macos"),
    all(feature = "windows-collector", target_os = "windows"),
)))]
impl NativeCollector {
    pub fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        Err(CollectorError::UnsupportedPlatform {
            platform: std::env::consts::OS,
        })
    }
}

#[derive(Debug)]
pub enum CollectorError {
    Io {
        context: &'static str,
        source: std::io::Error,
    },
    #[cfg(all(feature = "linux-collector", target_os = "linux"))]
    Procfs(procfs::ProcError),
    Parse {
        context: &'static str,
        message: String,
    },
    UnsupportedPlatform {
        platform: &'static str,
    },
}

impl CollectorError {
    pub fn parse(context: &'static str, message: impl Into<String>) -> Self {
        Self::Parse {
            context,
            message: message.into(),
        }
    }
}

impl fmt::Display for CollectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(formatter, "{context}: {source}"),
            #[cfg(all(feature = "linux-collector", target_os = "linux"))]
            Self::Procfs(error) => write!(formatter, "{error}"),
            Self::Parse { context, message } => write!(formatter, "{context}: {message}"),
            Self::UnsupportedPlatform { platform } => {
                write!(formatter, "collector is not supported on {platform}")
            }
        }
    }
}

impl std::error::Error for CollectorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            #[cfg(all(feature = "linux-collector", target_os = "linux"))]
            Self::Procfs(error) => Some(error),
            Self::Parse { .. } | Self::UnsupportedPlatform { .. } => None,
        }
    }
}

#[cfg(all(feature = "linux-collector", target_os = "linux"))]
impl From<procfs::ProcError> for CollectorError {
    fn from(error: procfs::ProcError) -> Self {
        Self::Procfs(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_totals_counts_every_pid_and_keeps_the_newest() {
        assert_eq!(
            process_totals([3, 1, 2]),
            ProcessTotals {
                count: 3,
                last_pid: 3,
            }
        );
    }

    #[test]
    fn process_totals_of_nothing_is_zero() {
        assert_eq!(
            process_totals(std::iter::empty()),
            ProcessTotals {
                count: 0,
                last_pid: 0,
            }
        );
    }

    #[test]
    fn process_totals_ignores_ordering_and_truncation_of_the_caller() {
        let pids = (1..=50).collect::<Vec<u64>>();
        let all = process_totals(pids.iter().copied());
        let reversed = process_totals(pids.iter().copied().rev());
        let truncated = process_totals(pids.iter().copied().take(8));

        assert_eq!(all, reversed);
        assert_ne!(all, truncated);
    }
}
