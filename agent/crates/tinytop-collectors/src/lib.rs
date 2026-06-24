pub mod linux;

use std::fmt;

pub type CollectorResult<T> = Result<T, CollectorError>;

#[derive(Debug)]
pub enum CollectorError {
    Io {
        context: &'static str,
        source: std::io::Error,
    },
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
            Self::Procfs(error) => Some(error),
            Self::Parse { .. } | Self::UnsupportedPlatform { .. } => None,
        }
    }
}

impl From<procfs::ProcError> for CollectorError {
    fn from(error: procfs::ProcError) -> Self {
        Self::Procfs(error)
    }
}
