//! Audio device enumeration and lookup.

pub mod configuration;
pub mod discovery;
pub mod microphone;
pub mod platform;
pub mod speakers;

pub use configuration::{get_device_and_config, parse_audio_device, AudioDevice, DeviceType};
pub use discovery::list_audio_devices;
pub use microphone::default_input_device;
pub use speakers::default_output_device;
