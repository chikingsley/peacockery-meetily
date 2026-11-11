//! Platform-specific device enumeration (Windows ships; Linux is for development).

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub use windows::{configure_windows_audio, get_windows_device};

#[cfg(target_os = "linux")]
pub use linux::configure_linux_audio;
