//! Non-Windows build: process loopback does not exist.

use crate::{AppLoopbackError, ProcessEntry};

pub(crate) fn windows_build() -> Option<u32> {
    None
}

pub(crate) fn process_snapshot() -> Option<Vec<ProcessEntry>> {
    None
}

/// Uninhabited: a capture can never be started here.
pub(crate) enum Capture {}

impl Capture {
    pub(crate) fn start(
        _pid: u32,
        _on_samples: impl FnMut(&[f32]) + Send + 'static,
        _on_error: impl FnMut(String) + Send + 'static,
    ) -> Result<Capture, AppLoopbackError> {
        Err(AppLoopbackError::Unsupported)
    }
}
