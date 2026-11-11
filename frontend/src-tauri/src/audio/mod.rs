// src/audio/mod.rs
pub mod audio_processing;
pub mod capture;
pub mod constants;
pub mod device_monitor;
pub mod devices;
pub mod encode;
pub mod ffmpeg;
pub mod import;
pub mod incremental_saver;
pub mod level_monitor;
pub mod levels;
pub mod pipeline;
pub mod recording_commands;
pub mod recording_manager;
pub mod recording_preferences;
pub mod recording_saver;
pub mod recording_state;
pub mod recovery;
pub mod stream;

pub use devices::{
    default_input_device, default_output_device, get_device_and_config, list_audio_devices,
    parse_audio_device, AudioDevice, DeviceType,
};
pub use recording_state::{AudioChunk, AudioError, RecordingState};
