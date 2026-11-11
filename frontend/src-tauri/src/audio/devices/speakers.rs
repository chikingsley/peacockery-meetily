use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};

use super::configuration::{AudioDevice, DeviceType};

/// The default render device, captured through WASAPI loopback on Windows.
pub fn default_output_device() -> Result<AudioDevice> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("No default output device found"))?;
    Ok(AudioDevice::new(device.name()?, DeviceType::Output))
}
