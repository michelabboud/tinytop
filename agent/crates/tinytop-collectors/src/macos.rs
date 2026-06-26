use tinytop_types::{RuntimeKind, SystemSnapshot};

use crate::{Collector, CollectorResult, common::SysinfoCollector};

#[derive(Default)]
pub struct MacOsCollector {
    inner: SysinfoCollector,
}

impl MacOsCollector {
    pub fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        self.inner.collect(
            RuntimeKind::MacOs,
            "macos",
            "macOS",
            "native macOS collector using sysinfo",
        )
    }
}

impl Collector for MacOsCollector {
    fn collect(&mut self) -> CollectorResult<SystemSnapshot> {
        Self::collect(self)
    }
}
