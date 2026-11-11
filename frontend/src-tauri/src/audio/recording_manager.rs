//! Coordinates one recording session: capture streams → pipeline → recording saver, the
//! live sessions, the energy track and levels, plus the device monitor that drives mic
//! hot-swap.

use anyhow::Result;
use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::device_monitor::{AudioDeviceMonitor, DeviceEvent};
use super::devices::AudioDevice;
use super::levels::LevelsCallback;
use super::pipeline::{AudioPipelineManager, EnergyRecorder, PipelineOutputs, PipelineStats};
use super::recording_saver::{MeetingTarget, RecordingSaver};
use super::recording_state::{AudioChunk, RecordingState};
use super::stream::{AudioStream, AudioStreamManager, SystemSource};
use crate::live::{channels_for, report_unavailable, LiveConfig, LiveManager, LiveSink, LiveSummary};

/// Live transcription for a recording; `client` is `Err(message)` when it cannot run
/// (no API key …), which is reported on every session's status.
pub struct LiveSetup {
    pub config: LiveConfig,
    pub client: Result<peacockery_voice::VoiceClient, String>,
    pub sink: Arc<dyn LiveSink>,
    pub pool: Option<sqlx::SqlitePool>,
}

/// Result of a finished recording.
#[derive(Debug)]
pub struct StoppedRecording {
    pub audio_path: Result<PathBuf>,
    /// Active (pause-excluded) duration in seconds.
    pub duration_seconds: Option<f64>,
    pub live: Option<LiveSummary>,
    /// What each capture stream delivered (`None` when the pipeline task failed).
    pub stats: Option<PipelineStats>,
}

pub struct RecordingManager {
    state: Arc<RecordingState>,
    stream_manager: AudioStreamManager,
    pipeline_manager: AudioPipelineManager,
    recording_saver: RecordingSaver,
    live: Option<LiveManager>,
    device_monitor: AudioDeviceMonitor,
    device_event_receiver: Option<mpsc::UnboundedReceiver<DeviceEvent>>,
}

// SAFETY: the cpal streams inside `AudioStreamManager` are only touched through the
// manager, which lives behind the global `RECORDING_MANAGER` mutex.
unsafe impl Send for RecordingManager {}

impl RecordingManager {
    /// Start capture for `target`. On error everything started so far is torn down.
    /// Live sessions connect in the background; start never waits for them.
    pub async fn start(
        target: MeetingTarget,
        microphone_device: Option<Arc<AudioDevice>>,
        system_source: Option<SystemSource>,
        live: Option<LiveSetup>,
        levels: LevelsCallback,
    ) -> Result<Self> {
        let state = RecordingState::new();
        let (recording_sender, recording_receiver) = mpsc::unbounded_channel::<AudioChunk>();
        let recording_saver = RecordingSaver::start(target.clone(), recording_receiver)?;
        state.start_recording()?;

        // Streams first (into the not-yet-ready pipeline, chunks are skipped), so the live
        // sessions know which sources exist.
        let mut stream_manager = AudioStreamManager::new(state.clone());
        let monitored_system = system_source.as_ref().and_then(SystemSource::endpoint);
        if let Err(e) = stream_manager.start_streams(microphone_device.clone(), system_source).await {
            state.stop_recording();
            if let Err(stop_error) = stream_manager.stop_streams() {
                warn!("Failed to stop streams after start error: {}", stop_error);
            }
            return Err(e);
        }

        let (live_manager, taps) = match live {
            Some(mut setup) => {
                // The pipeline starts right below: this is recording time 0.
                setup.config.started_at = chrono::Local::now();
                let channels = channels_for(setup.config.mode, stream_manager.has_microphone(), stream_manager.has_system());
                match setup.client {
                    Ok(client) => {
                        let (manager, taps) = LiveManager::start(
                            setup.config,
                            client,
                            setup.sink,
                            recording_saver.live_lines().clone(),
                            setup.pool,
                            &channels,
                        );
                        (Some(manager), Some(taps))
                    }
                    Err(message) => {
                        warn!("Live transcription unavailable: {}", message);
                        report_unavailable(setup.sink.as_ref(), &setup.config, &channels, &message);
                        (None, None)
                    }
                }
            }
            None => (None, None),
        };

        let mut pipeline_manager = AudioPipelineManager::new();
        pipeline_manager.start(
            &state,
            PipelineOutputs {
                recording: Some(recording_sender),
                live: taps,
                energy: Some(EnergyRecorder::new(target.folder.clone())),
                levels: Some(levels),
            },
        );

        let (mut device_monitor, device_event_receiver) = AudioDeviceMonitor::new();
        if let Err(e) = device_monitor.start_monitoring(microphone_device, monitored_system) {
            warn!("Failed to start device monitoring: {}", e);
        }

        info!("Recording manager started");
        Ok(Self {
            state,
            stream_manager,
            pipeline_manager,
            recording_saver,
            live: live_manager,
            device_monitor,
            device_event_receiver: Some(device_event_receiver),
        })
    }

    /// Stop capture and drain the pipeline (which closes the live inputs and writes the
    /// energy track), then write `audio.mp4` while the live sessions finish.
    pub async fn stop_and_save(&mut self) -> StoppedRecording {
        self.device_monitor.stop_monitoring().await;
        let duration_seconds = self.state.get_active_recording_duration();

        // Closes the capture channel: the pipeline drains, then drops every output.
        self.state.stop_recording();
        if let Err(e) = self.stream_manager.stop_streams() {
            error!("Error stopping audio streams: {}", e);
        }
        let stats = self.pipeline_manager.wait_for_drain().await;
        let live = self.live.take();
        let (audio_path, live) = tokio::join!(self.recording_saver.finish(), async move {
            match live {
                Some(live) => Some(live.finish().await),
                None => None,
            }
        });
        self.state.cleanup();
        StoppedRecording { audio_path, duration_seconds, live, stats }
    }
    pub fn target(&self) -> &MeetingTarget {
        self.recording_saver.target()
    }

    pub fn is_recording(&self) -> bool {
        self.state.is_recording()
    }

    pub fn pause_recording(&self) -> Result<()> {
        self.state.pause_recording()
    }

    pub fn resume_recording(&self) -> Result<()> {
        self.state.resume_recording()
    }

    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }

    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    pub fn get_recording_duration(&self) -> Option<f64> {
        self.state.get_recording_duration()
    }

    pub fn get_active_recording_duration(&self) -> Option<f64> {
        self.state.get_active_recording_duration()
    }

    pub fn get_total_pause_duration(&self) -> f64 {
        self.state.get_total_pause_duration()
    }

    pub fn get_current_pause_duration(&self) -> Option<f64> {
        self.state.get_current_pause_duration()
    }

    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&super::recording_state::AudioError) + Send + Sync + 'static,
    {
        self.state.set_error_callback(callback);
    }

    /// Finalized live lines of this session (`get_transcript_history`).
    pub fn live_lines(&self) -> Vec<crate::live::LiveLine> {
        self.recording_saver.live_lines().lines()
    }

    /// Hand the device event stream to the background hot-swap processor.
    pub fn take_device_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<DeviceEvent>> {
        self.device_event_receiver.take()
    }

    /// Take the mic stream out for hot-swap so the caller can tear it down without
    /// holding `RECORDING_MANAGER`. System audio keeps running.
    pub fn take_mic_stream_for_swap(&mut self) -> Option<AudioStream> {
        self.stream_manager.take_mic_stream()
    }

    /// Install the replacement mic stream after a hot-swap.
    pub fn set_mic_stream_after_swap(
        &mut self,
        stream: AudioStream,
        device: Arc<AudioDevice>,
        system_name: Option<String>,
    ) {
        self.stream_manager.set_mic_stream(stream);
        self.device_monitor.notify_mic_swapped(device.name.clone(), system_name);
    }

    pub fn get_state(&self) -> &Arc<RecordingState> {
        &self.state
    }
}

impl Drop for RecordingManager {
    fn drop(&mut self) {
        self.state.cleanup();
    }
}
