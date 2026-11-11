use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, SupportedStreamConfig};
use log::{error, info, warn};

use super::devices::{AudioDevice, get_device_and_config};
use super::capture::AudioCapture;
use super::recording_state::{RecordingState, DeviceType};

/// One cpal input stream (a capture device, or a render device opened as WASAPI loopback).
pub struct AudioStream {
    device: Arc<AudioDevice>,
    stream: Stream,
}

// SAFETY: cpal's WASAPI/ALSA streams are only paused and dropped through this wrapper;
// the stream is never shared between threads concurrently.
unsafe impl Send for AudioStream {}

impl AudioStream {
    /// Open and start a cpal stream for `device`.
    pub async fn create(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
    ) -> Result<Self> {
        info!("Creating {:?} stream for device: {}", device_type, device.name);
        let (cpal_device, config) = get_device_and_config(&device).await?;
        info!(
            "Audio config - Sample rate: {}, Channels: {}, Format: {:?}",
            config.sample_rate().0,
            config.channels(),
            config.sample_format()
        );

        let capture = AudioCapture::new(
            device.clone(),
            state,
            config.sample_rate().0,
            config.channels(),
            device_type,
        );
        let stream = Self::build_stream(&cpal_device, &config, capture)?;
        stream.play()?;
        info!("Stream started for device: {}", device.name);
        Ok(Self { device, stream })
    }

    /// Build stream based on sample format
    fn build_stream(
        device: &Device,
        config: &SupportedStreamConfig,
        capture: AudioCapture,
    ) -> Result<Stream> {
        let config_copy = config.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        capture.process_audio_data(data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i16::MAX as f32)
                            .collect();
                        capture.process_audio_data(&f32_data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I32 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i32::MAX as f32)
                            .collect();
                        capture.process_audio_data(&f32_data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I8 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i8], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i8::MAX as f32)
                            .collect();
                        capture.process_audio_data(&f32_data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported sample format: {:?}", config.sample_format()));
            }
        };

        Ok(stream)
    }

    /// Stop the stream
    pub fn stop(self) -> Result<()> {
        info!("Stopping audio stream for device: {}", self.device.name);

        // CRITICAL: Pause the stream first to stop callbacks immediately
        // This ensures closures stop executing before we drop the stream,
        // allowing Arc references captured in callbacks to be released
        if let Err(e) = self.stream.pause() {
            warn!("Failed to pause stream before drop: {}", e);
        }
        info!("Stream paused, now dropping to release callbacks");
        drop(self.stream);

        // Explicitly drop self.device Arc reference
        drop(self.device);
        info!("Audio stream stopped and device reference dropped");
        Ok(())
    }
}

/// Name of the pseudo system device: process-tree loopback of Google Chrome.
pub const CHROME_APP_DEVICE: &str = "app:chrome";
pub const CHROME_APP_LABEL: &str = "Google Chrome app audio (Google Voice)";
const CHROME_EXE: &str = "chrome.exe";
pub const CHROME_NOT_RUNNING: &str = "Google Chrome is not running — open Google Voice first";
const CHROME_UNSUPPORTED: &str = "Google Chrome app audio needs Windows 10 build 20348 or newer";

/// Where system audio comes from.
#[derive(Debug, Clone)]
pub enum SystemSource {
    /// A render endpoint opened as WASAPI loopback (Linux: a monitor source).
    Endpoint(Arc<AudioDevice>),
    /// Process-tree loopback of the Chrome browser (Google Voice PWA).
    ChromeApp,
}

impl SystemSource {
    /// `app:chrome` → `ChromeApp`, anything else is an endpoint.
    pub fn for_device(device: Arc<AudioDevice>) -> Self {
        if device.name == CHROME_APP_DEVICE {
            SystemSource::ChromeApp
        } else {
            SystemSource::Endpoint(device)
        }
    }

    /// The endpoint the device monitor should watch (none for the Chrome capture).
    pub fn endpoint(&self) -> Option<Arc<AudioDevice>> {
        match self {
            SystemSource::Endpoint(device) => Some(device.clone()),
            SystemSource::ChromeApp => None,
        }
    }

    /// Fails early when the source cannot be opened (Chrome not running / unsupported OS).
    pub fn check(&self) -> Result<()> {
        match self {
            SystemSource::Endpoint(_) => Ok(()),
            SystemSource::ChromeApp => chrome_root_pid().map(|_| ()),
        }
    }
}

fn chrome_root_pid() -> Result<u32> {
    if !app_loopback::is_supported() {
        return Err(anyhow::anyhow!(CHROME_UNSUPPORTED));
    }
    app_loopback::find_process_tree_root(CHROME_EXE).ok_or_else(|| anyhow::anyhow!(CHROME_NOT_RUNNING))
}

/// A running system-audio capture.
pub enum SystemStream {
    Endpoint(AudioStream),
    ChromeApp(app_loopback::AppLoopbackCapture),
}

impl SystemStream {
    pub async fn open(source: &SystemSource, state: Arc<RecordingState>) -> Result<Self> {
        match source {
            SystemSource::Endpoint(device) => {
                AudioStream::create(device.clone(), state, DeviceType::System).await.map(SystemStream::Endpoint)
            }
            SystemSource::ChromeApp => {
                let pid = chrome_root_pid()?;
                info!("Capturing Google Chrome process tree (root pid {})", pid);
                let device = Arc::new(AudioDevice::new(CHROME_APP_DEVICE.to_string(), super::devices::DeviceType::Output));
                let capture = AudioCapture::new(device, state, app_loopback::SAMPLE_RATE, 1, DeviceType::System);
                let on_error = capture.clone();
                // Activation blocks up to 10 s: keep it off the async runtime.
                let started = tokio::task::spawn_blocking(move || {
                    app_loopback::AppLoopbackCapture::start(
                        pid,
                        move |samples| capture.process_audio_data(samples),
                        move |message| on_error.report_source_ended(&message),
                    )
                })
                .await
                .map_err(|e| anyhow::anyhow!("Chrome capture task failed: {}", e))?;
                match started {
                    Ok(capture) => Ok(SystemStream::ChromeApp(capture)),
                    Err(app_loopback::AppLoopbackError::Unsupported) => Err(anyhow::anyhow!(CHROME_UNSUPPORTED)),
                    Err(app_loopback::AppLoopbackError::ProcessNotFound(_)) => Err(anyhow::anyhow!(CHROME_NOT_RUNNING)),
                    Err(e) => Err(anyhow::anyhow!("Could not capture Google Chrome audio: {}", e)),
                }
            }
        }
    }

    pub fn stop(self) -> Result<()> {
        match self {
            SystemStream::Endpoint(stream) => stream.stop(),
            SystemStream::ChromeApp(capture) => {
                capture.stop();
                info!("Chrome audio capture stopped");
                Ok(())
            }
        }
    }
}

/// The capture streams of one session.
pub struct AudioStreamManager {
    microphone_stream: Option<AudioStream>,
    system_stream: Option<SystemStream>,
    state: Arc<RecordingState>,
}

// SAFETY: AudioStream is Send (see above) and AppLoopbackCapture is Send.
unsafe impl Send for AudioStreamManager {}

impl AudioStreamManager {
    pub fn new(state: Arc<RecordingState>) -> Self {
        Self { microphone_stream: None, system_stream: None, state }
    }

    /// Start the mic and the system source. A failing mic fails the start; a failing
    /// system source does too when it is the Chrome capture (the user picked it on purpose),
    /// while a failing endpoint only logs (the call is still recorded from the mic).
    pub async fn start_streams(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_source: Option<SystemSource>,
    ) -> Result<()> {
        if let Some(mic_device) = microphone_device {
            info!("Creating microphone stream: {}", mic_device.name);
            let stream = AudioStream::create(mic_device, self.state.clone(), DeviceType::Microphone).await?;
            self.microphone_stream = Some(stream);
        }
        if let Some(source) = system_source {
            match tokio::time::timeout(Duration::from_secs(15), SystemStream::open(&source, self.state.clone())).await {
                Ok(Ok(stream)) => self.system_stream = Some(stream),
                Ok(Err(e)) if matches!(source, SystemSource::ChromeApp) => return Err(e),
                Ok(Err(e)) => warn!("Failed to create system audio stream: {}", e),
                Err(_) if matches!(source, SystemSource::ChromeApp) => {
                    return Err(anyhow::anyhow!("Google Chrome audio capture did not start within 15 s"))
                }
                Err(_) => warn!("System audio stream did not start within 15 s"),
            }
        }
        if self.microphone_stream.is_none() && self.system_stream.is_none() {
            return Err(anyhow::anyhow!("No audio streams could be created"));
        }
        Ok(())
    }

    pub fn has_microphone(&self) -> bool {
        self.microphone_stream.is_some()
    }

    pub fn has_system(&self) -> bool {
        self.system_stream.is_some()
    }

    /// Stop all audio streams
    pub fn stop_streams(&mut self) -> Result<()> {
        let mut errors = Vec::new();
        if let Some(mic_stream) = self.microphone_stream.take() {
            if let Err(e) = mic_stream.stop() {
                error!("Failed to stop microphone stream: {}", e);
                errors.push(e);
            }
        }
        if let Some(sys_stream) = self.system_stream.take() {
            if let Err(e) = sys_stream.stop() {
                error!("Failed to stop system stream: {}", e);
                errors.push(e);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to stop some streams: {:?}", errors))
        }
    }

    /// Take the microphone stream OUT of the manager, keeping system audio
    /// running. The caller stops/drops it OUTSIDE any lock — a cpal teardown
    /// of a dead BT device can stall and must not block a held mutex.
    pub fn take_mic_stream(&mut self) -> Option<AudioStream> {
        self.microphone_stream.take()
    }

    /// Set a new microphone stream (used after hot-swap creation).
    pub fn set_mic_stream(&mut self, stream: AudioStream) {
        self.microphone_stream = Some(stream);
    }
}

impl Drop for AudioStreamManager {
    fn drop(&mut self) {
        if let Err(e) = self.stop_streams() {
            error!("Error stopping streams during drop: {}", e);
        }
    }
}
