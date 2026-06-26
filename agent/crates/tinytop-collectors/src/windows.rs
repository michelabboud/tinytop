use tinytop_types::{RuntimeKind, SystemSnapshot};

use crate::{Collector, CollectorResult, common::SysinfoCollector};

#[derive(Default)]
pub struct WindowsCollector {
    inner: SysinfoCollector,
}

impl WindowsCollector {
    pub fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        self.inner.collect(
            RuntimeKind::Windows,
            "windows",
            "Windows",
            "native Windows collector using sysinfo",
        )
    }
}

impl Collector for WindowsCollector {
    fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        Self::collect(self)
    }
}
