use std::sync::Mutex;

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use futures_util::future::{BoxFuture, Shared};
use futures_util::FutureExt;

use super::configuration::{AudioDevice, DeviceType};
use super::platform;

type Scan = Shared<BoxFuture<'static, Result<Vec<AudioDevice>, String>>>;

#[derive(Default)]
struct DeviceDiscovery(Mutex<Option<Scan>>);

impl DeviceDiscovery {
    /// Joining an in-flight scan bounds native work even if its original caller cancels.
    async fn scan(
        &self,
        enumerate: impl FnOnce() -> Result<Vec<AudioDevice>> + Send + 'static,
    ) -> Result<Vec<AudioDevice>> {
        let scan = {
            let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if slot.as_ref().is_none_or(|scan| scan.peek().is_some()) {
                let worker = tokio::task::spawn_blocking(enumerate);
                *slot = Some(
                    async move {
                        worker
                            .await
                            .map_err(|e| e.to_string())?
                            .map_err(|e| e.to_string())
                    }
                    .boxed()
                    .shared(),
                );
            }
            slot.as_ref().unwrap().clone()
        };
        scan.await.map_err(anyhow::Error::msg)
    }
}

static DISCOVERY: DeviceDiscovery = DeviceDiscovery(Mutex::new(None));

pub async fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    DISCOVERY.scan(enumerate_audio_devices).await
}

fn enumerate_audio_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();

    // Platform-specific device enumeration
    let mut devices = {
        #[cfg(target_os = "windows")]
        {
            platform::configure_windows_audio(&host)?
        }

        #[cfg(target_os = "linux")]
        {
            platform::configure_linux_audio(&host)?
        }
    };

    // Add any additional devices from the default host
    if let Ok(other_devices) = host.devices() {
        for device in other_devices {
            if let Ok(name) = device.name() {
                if !devices.iter().any(|d| d.name == name) {
                    devices.push(AudioDevice::new(name, DeviceType::Output));
                }
            }
        }
    }

    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelled_caller_does_not_duplicate_a_stalled_driver_scan() {
        let discovery = Arc::new(DeviceDiscovery::default());
        let (started, scanning) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let first = tokio::spawn({
            let discovery = discovery.clone();
            async move {
                discovery
                    .scan(move || {
                        started.send(()).unwrap();
                        wait.recv_timeout(Duration::from_secs(3))
                            .expect("async runtime stalled");
                        Ok(Vec::new())
                    })
                    .await
            }
        });
        scanning.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());

        let second = discovery.scan(|| panic!("started a duplicate native scan"));
        tokio::pin!(second);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut second)
            .await
            .is_err());
        release.send(()).unwrap();
        assert!(second.await.unwrap().is_empty());
        let fresh = discovery
            .scan(|| Ok(vec![AudioDevice::new("USB".into(), DeviceType::Input)]))
            .await
            .unwrap();
        assert_eq!(fresh[0].name, "USB");
    }
}
