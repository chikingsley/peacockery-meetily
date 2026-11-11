//! Settings device test: opens the selected devices with the recording capture code and
//! emits real `audio-levels` (~10 Hz) until stopped. Nothing is recorded.

use anyhow::{anyhow, Result};
use log::{info, warn};
use peacockery_voice::rms_dbfs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::devices::{parse_audio_device, DeviceType as Direction};
use super::levels::LevelMeter;
use super::recording_state::{AudioChunk, DeviceType, RecordingState};
use super::stream::{AudioStream, SystemSource, SystemStream};

const FRAME: Duration = Duration::from_millis(100);

struct Monitor {
    state: Arc<RecordingState>,
    mic: Option<AudioStream>,
    system: Option<SystemStream>,
    task: JoinHandle<()>,
}

// SAFETY: the cpal stream is only stopped through `stop_monitoring`, never shared.
unsafe impl Send for Monitor {}

static MONITOR: Mutex<Option<Monitor>> = Mutex::new(None);

/// Open `device_names` ("<name> (input)" / "<name> (output)", `app:chrome (output)`) and
/// start emitting their levels. Replaces a running monitor.
pub async fn start_monitoring<R: Runtime>(app: AppHandle<R>, device_names: Vec<String>) -> Result<()> {
    stop_monitoring();
    let mut mic_device = None;
    let mut system_source = None;
    for name in &device_names {
        let device = Arc::new(parse_audio_device(name)?);
        match device.device_type {
            Direction::Input if mic_device.is_none() => mic_device = Some(device),
            Direction::Output if system_source.is_none() => system_source = Some(SystemSource::for_device(device)),
            _ => warn!("Level test: ignoring extra device '{}'", name),
        }
    }

    let state = RecordingState::new();
    let (sender, receiver) = mpsc::unbounded_channel::<AudioChunk>();
    state.set_audio_sender(sender);
    state.start_recording()?;

    let mic = match mic_device {
        Some(device) => Some(AudioStream::create(device, state.clone(), DeviceType::Microphone).await?),
        None => None,
    };
    let system = match &system_source {
        Some(source) => match SystemStream::open(source, state.clone()).await {
            Ok(stream) => Some(stream),
            Err(e) => {
                state.stop_recording();
                if let Some(mic) = mic {
                    let _ = mic.stop();
                }
                return Err(e);
            }
        },
        None => None,
    };
    if mic.is_none() && system.is_none() {
        return Err(anyhow!("No device to test"));
    }
    info!("Level test started for {:?}", device_names);

    let task = tokio::spawn(emit_levels(app, receiver));
    *MONITOR.lock().unwrap_or_else(|p| p.into_inner()) = Some(Monitor { state, mic, system, task });
    Ok(())
}

async fn emit_levels<R: Runtime>(app: AppHandle<R>, mut receiver: mpsc::UnboundedReceiver<AudioChunk>) {
    let mut meter = LevelMeter::default();
    let mut mic = Vec::new();
    let mut sys = Vec::new();
    let mut tick = tokio::time::interval(FRAME);
    loop {
        tokio::select! {
            chunk = receiver.recv() => match chunk {
                Some(chunk) => match chunk.device_type {
                    DeviceType::Microphone => mic.extend_from_slice(&chunk.data),
                    DeviceType::System => sys.extend_from_slice(&chunk.data),
                },
                None => break,
            },
            _ = tick.tick() => {
                let levels = meter.frame(rms_dbfs(&mic), rms_dbfs(&sys), FRAME.as_secs_f64());
                mic.clear();
                sys.clear();
                if app.emit("audio-levels", levels).is_err() {
                    break;
                }
            }
        }
    }
}

/// Close the test devices (no-op when nothing runs).
pub fn stop_monitoring() {
    let Some(monitor) = MONITOR.lock().unwrap_or_else(|p| p.into_inner()).take() else {
        return;
    };
    monitor.state.stop_recording();
    if let Some(mic) = monitor.mic {
        if let Err(e) = mic.stop() {
            warn!("Level test: failed to stop the microphone: {}", e);
        }
    }
    if let Some(system) = monitor.system {
        if let Err(e) = system.stop() {
            warn!("Level test: failed to stop system audio: {}", e);
        }
    }
    monitor.task.abort();
    info!("Level test stopped");
}
