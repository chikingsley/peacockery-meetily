use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};

use super::configuration::{AudioDevice, DeviceType};

/// The default capture device (WASAPI eConsole default on Windows).
pub fn default_input_device() -> Result<AudioDevice> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("No default input device found"))?;
    Ok(AudioDevice::new(device.name()?, DeviceType::Input))
}
