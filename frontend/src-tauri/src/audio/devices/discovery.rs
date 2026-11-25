use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use futures_util::future::{BoxFuture, Shared};
use futures_util::FutureExt;
use tokio::sync::Semaphore;
use tokio::time::{timeout_at, Instant};

use super::configuration::{AudioDevice, DeviceType};
use super::platform;

type Scan = Shared<BoxFuture<'static, Result<Vec<AudioDevice>, String>>>;

struct DeviceDiscovery {
    active: Mutex<Option<Scan>>,
    workers: Arc<Semaphore>,
    deadline: Duration,
}

impl DeviceDiscovery {
    fn new(deadline: Duration) -> Self {
        Self {
            active: Mutex::new(None),
            // A timed-out driver call can keep its native thread. Permit one recovery scan.
            workers: Arc::new(Semaphore::new(2)),
            deadline,
        }
    }

    /// Concurrent callers share the scan even if its original caller cancels.
    async fn scan(
        &self,
        enumerate: impl FnOnce() -> Result<Vec<AudioDevice>> + Send + 'static,
    ) -> Result<Vec<AudioDevice>> {
        let scan = {
            let mut slot = self.active.lock().unwrap_or_else(|e| e.into_inner());
            if slot.as_ref().is_none_or(|scan| scan.peek().is_some()) {
                let permit = self.workers.clone().try_acquire_owned().map_err(|_| {
                    anyhow::anyhow!("Audio driver is still processing two timed-out device scans")
                })?;
                let expires = Instant::now() + self.deadline;
                let worker = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    enumerate()
                });
                *slot = Some(
                    async move {
                        timeout_at(expires, worker)
                            .await
                            .map_err(|_| "Audio device enumeration timed out".to_owned())?
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

static DISCOVERY: LazyLock<DeviceDiscovery> =
    LazyLock::new(|| DeviceDiscovery::new(Duration::from_secs(10)));

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

    #[tokio::test]
    async fn a_timed_out_driver_scan_allows_recovery_and_limits_blocked_threads() {
        let discovery = DeviceDiscovery::new(Duration::from_millis(50));
        let (release_first, wait_first) = std::sync::mpsc::channel();
        let first = discovery
            .scan(move || {
                let _ = wait_first.recv_timeout(Duration::from_secs(3));
                Ok(vec![])
            })
            .await;
        assert!(first.unwrap_err().to_string().contains("timed out"));
        let recovered = discovery
            .scan(|| Ok(vec![AudioDevice::new("USB".into(), DeviceType::Input)]))
            .await
            .unwrap();
        assert_eq!(recovered[0].name, "USB");

        let (release_second, wait_second) = std::sync::mpsc::channel();
        let second = discovery
            .scan(move || {
                let _ = wait_second.recv_timeout(Duration::from_secs(3));
                Ok(vec![])
            })
            .await;
        assert!(second.unwrap_err().to_string().contains("timed out"));
        let capped = discovery
            .scan(|| panic!("started a third native scan"))
            .await;
        assert!(capped.unwrap_err().to_string().contains("two timed-out"));
        release_first.send(()).unwrap();
        release_second.send(()).unwrap();
    }

    #[tokio::test]
    async fn cancelled_caller_does_not_duplicate_a_stalled_driver_scan() {
        let discovery = Arc::new(DeviceDiscovery::new(Duration::from_secs(2)));
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
