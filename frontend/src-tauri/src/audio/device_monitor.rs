//! Watches the recording microphone and reports when it disappears, which drives the mic
//! fallback to the system default input (`recording_commands`).
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::devices::{list_audio_devices, AudioDevice};

/// How often the device list is read.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The monitored microphone has been missing for its disconnect threshold.
#[derive(Debug, Clone)]
pub struct MicDisconnected {
    pub device_name: String,
}

/// Monitor state for the microphone.
#[derive(Debug, Clone)]
struct MonitoredMic {
    name: String,
    consecutive_missing: u32,
    is_bluetooth: bool,
}

impl MonitoredMic {
    fn new(name: String) -> Self {
        // Heuristic: Bluetooth devices drop out briefly, so they get more grace.
        let lower = name.to_lowercase();
        let is_bluetooth =
            lower.contains("airpods") || lower.contains("bluetooth") || lower.contains("wireless");
        Self {
            name,
            consecutive_missing: 0,
            is_bluetooth,
        }
    }

    /// Missing polls before a disconnect is reported.
    fn disconnect_threshold(&self) -> u32 {
        if self.is_bluetooth {
            3 // 6 s
        } else {
            2 // 4 s
        }
    }

    /// Record one poll; true when a disconnect should be reported. While the device stays
    /// missing this repeats every threshold polls: a failed fallback leaves the dead device
    /// tracked, so the repeat retries it (attempts are bounded by the fallback).
    fn observe(&mut self, present: bool) -> bool {
        if present {
            if self.consecutive_missing > 0 {
                info!(
                    "Microphone '{}' is back after {} missing checks",
                    self.name, self.consecutive_missing
                );
                self.consecutive_missing = 0;
            }
            return false;
        }
        self.consecutive_missing += 1;
        debug!(
            "Microphone '{}' missing for {} checks (threshold: {})",
            self.name,
            self.consecutive_missing,
            self.disconnect_threshold()
        );
        self.consecutive_missing % self.disconnect_threshold() == 0
    }
}

/// New microphone name after a hot-swap, picked up on the next poll.
type MicUpdateMailbox = Arc<std::sync::Mutex<Option<String>>>;

/// Polls the device list for the recording microphone.
pub struct AudioDeviceMonitor {
    monitor_handle: Option<JoinHandle<()>>,
    event_sender: mpsc::UnboundedSender<MicDisconnected>,
    stop_signal: Arc<tokio::sync::Notify>,
    mic_update_mailbox: MicUpdateMailbox,
}

impl AudioDeviceMonitor {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<MicDisconnected>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        (
            Self {
                monitor_handle: None,
                event_sender,
                stop_signal: Arc::new(tokio::sync::Notify::new()),
                mic_update_mailbox: Arc::default(),
            },
            event_receiver,
        )
    }

    /// The mic was hot-swapped: watch `new_mic_name` from the next poll on.
    pub fn notify_mic_swapped(&self, new_mic_name: String) {
        if let Ok(mut mailbox) = self.mic_update_mailbox.lock() {
            *mailbox = Some(new_mic_name);
        }
    }

    pub fn start_monitoring(&mut self, microphone: Arc<AudioDevice>) {
        if self.monitor_handle.is_some() {
            warn!("Device monitor already running");
            return;
        }
        let mic = MonitoredMic::new(microphone.name.clone());
        info!(
            "Monitoring microphone: '{}' (Bluetooth: {})",
            mic.name, mic.is_bluetooth
        );
        let event_sender = self.event_sender.clone();
        let stop_signal = self.stop_signal.clone();
        let mailbox = self.mic_update_mailbox.clone();
        self.monitor_handle = Some(tokio::spawn(async move {
            Self::monitor_loop(mic, event_sender, stop_signal, mailbox).await;
        }));
    }

    pub async fn stop_monitoring(&mut self) {
        self.stop_signal.notify_one();
        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.await;
            info!("Device monitor stopped");
        }
    }

    async fn monitor_loop(
        mut mic: MonitoredMic,
        event_sender: mpsc::UnboundedSender<MicDisconnected>,
        stop_signal: Arc<tokio::sync::Notify>,
        mailbox: MicUpdateMailbox,
    ) {
        loop {
            tokio::select! {
                _ = stop_signal.notified() => break,
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
            }

            let current_devices = match list_audio_devices().await {
                Ok(devices) => devices,
                Err(e) => {
                    error!("Failed to list audio devices: {}", e);
                    continue;
                }
            };

            // A completed hot-swap retargets the monitor and resets the missing count.
            if let Some(new_name) = mailbox.lock().ok().and_then(|mut m| m.take()) {
                info!(
                    "[DEVICE_MONITOR] Updated tracked mic: '{}' → '{}' (hot-swap)",
                    mic.name, new_name
                );
                mic = MonitoredMic::new(new_name);
            }

            let present = current_devices.iter().any(|d| d.name == mic.name);
            if mic.observe(present) {
                warn!("Microphone '{}' disconnected", mic.name);
                let _ = event_sender.send(MicDisconnected {
                    device_name: mic.name.clone(),
                });
            }
        }
    }
}

impl Drop for AudioDeviceMonitor {
    fn drop(&mut self) {
        self.stop_signal.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_mics_get_more_grace() {
        let airpods = MonitoredMic::new("John's AirPods Pro".to_string());
        assert!(airpods.is_bluetooth);
        assert_eq!(airpods.disconnect_threshold(), 3);

        let builtin = MonitoredMic::new("Built-in Microphone".to_string());
        assert!(!builtin.is_bluetooth);
        assert_eq!(builtin.disconnect_threshold(), 2);
    }

    #[test]
    fn a_missing_mic_is_reported_at_its_threshold_and_again_while_missing() {
        let mut mic = MonitoredMic::new("USB Microphone".to_string());
        assert!(!mic.observe(true));
        assert!(!mic.observe(false));
        assert!(mic.observe(false), "reported after two missing polls");
        assert!(!mic.observe(false));
        assert!(mic.observe(false), "reported again while still missing");
        assert!(!mic.observe(true));
        assert_eq!(mic.consecutive_missing, 0);
        assert!(!mic.observe(false), "a returned mic starts counting again");
    }

    #[tokio::test]
    async fn stopping_an_unstarted_monitor_is_safe() {
        let (mut monitor, _receiver) = AudioDeviceMonitor::new();
        assert!(monitor.monitor_handle.is_none());
        monitor.stop_monitoring().await;
    }
}
