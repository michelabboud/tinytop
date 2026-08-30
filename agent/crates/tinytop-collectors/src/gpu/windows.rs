use super::GpuBackend;

/// Task 16 owns the native Windows PDH + DXGI backend; it is not implemented yet.
pub fn detect_backend() -> Option<Box<dyn GpuBackend>> {
    None
}
