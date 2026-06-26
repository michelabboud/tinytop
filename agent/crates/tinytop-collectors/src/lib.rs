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

use std::fmt;

use tinytop_types::SystemSnapshot;

pub type CollectorResult<T> = Result<T, CollectorError>;

pub trait Collector {
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
