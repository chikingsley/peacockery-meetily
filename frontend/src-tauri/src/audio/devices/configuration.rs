use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Eq, PartialEq, Hash, Serialize, Debug, Deserialize)]
pub enum DeviceType {
    Input,
    Output,
}

#[derive(Clone, Eq, PartialEq, Hash, Serialize, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub device_type: DeviceType,
    /// Display label for pseudo devices (`app:chrome`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl AudioDevice {
    pub fn new(name: String, device_type: DeviceType) -> Self {
        AudioDevice { name, device_type, label: None }
    }

    pub fn with_label(name: &str, device_type: DeviceType, label: &str) -> Self {
        AudioDevice { name: name.to_string(), device_type, label: Some(label.to_string()) }
    }

    pub fn from_name(name: &str) -> Result<Self> {
        if name.trim().is_empty() {
            return Err(anyhow!("Device name cannot be empty"));
        }

        let (name, device_type) = if name.to_lowercase().ends_with("(input)") {
            (
                name.trim_end_matches("(input)").trim().to_string(),
                DeviceType::Input,
            )
        } else if name.to_lowercase().ends_with("(output)") {
            (
                name.trim_end_matches("(output)").trim().to_string(),
                DeviceType::Output,
            )
        } else {
            return Err(anyhow!(
                "Device type (input/output) not specified in the name"
            ));
        };

        Ok(AudioDevice::new(name, device_type))
    }
}

impl fmt::Display for AudioDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.name,
            match self.device_type {
                DeviceType::Input => "input",
                DeviceType::Output => "output",
            }
        )
    }
}

/// Parse audio device from string name
pub fn parse_audio_device(name: &str) -> Result<AudioDevice> {
    AudioDevice::from_name(name)
}

/// The cpal device and its default (mix-format) config for `audio_device`.
#[cfg(target_os = "windows")]
pub async fn get_device_and_config(
    audio_device: &AudioDevice,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig)> {
    super::platform::get_windows_device(audio_device)
}

/// The cpal device and its default config for `audio_device` (Linux development:
/// system audio comes from PulseAudio/PipeWire monitor sources exposed as ALSA inputs).
#[cfg(not(target_os = "windows"))]
pub async fn get_device_and_config(
    audio_device: &AudioDevice,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig)> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = match audio_device.device_type {
        DeviceType::Input => cpal::default_host(),
        DeviceType::Output => cpal::host_from_id(cpal::HostId::Alsa)
            .map_err(|e| anyhow!("ALSA host unavailable: {}", e))?,
    };
    for device in host.input_devices()? {
        if device.name().is_ok_and(|name| name == audio_device.name) {
            let config = device
                .default_input_config()
                .map_err(|e| anyhow!("Failed to get default input config: {}", e))?;
            return Ok((device, config));
        }
    }
    Err(anyhow!("Device not found: {}", audio_device.name))
}
