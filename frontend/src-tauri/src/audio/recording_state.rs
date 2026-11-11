use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Device type for audio chunks
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Microphone,
    System,
}

/// Mono 48 kHz samples from one capture source (or the mix, towards the saver).
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub data: Vec<f32>,
    pub device_type: DeviceType,
}

/// Comprehensive error types for audio system
#[derive(Debug, Clone)]
pub enum AudioError {
    DeviceDisconnected,
    StreamFailed,
    ProcessingFailed,
    ChannelClosed,
    InitializationFailed,
    ConfigurationError,
    PermissionDenied,
    BufferOverflow,
    SampleRateUnsupported,
    /// The Chrome process loopback ended (Chrome closed or the stream failed).
    AppAudioEnded,
}

impl AudioError {
    /// Check if error is recoverable (can attempt reconnection)
    pub fn is_recoverable(&self) -> bool {
        match self {
            // Device disconnect is now recoverable - we can attempt reconnection
            AudioError::DeviceDisconnected => true,
            AudioError::StreamFailed => true,
            AudioError::ProcessingFailed => true,
            AudioError::ChannelClosed => false,
            AudioError::InitializationFailed => false,
            AudioError::ConfigurationError => false,
            AudioError::PermissionDenied => false,
            AudioError::BufferOverflow => true,
            AudioError::SampleRateUnsupported => false,
            AudioError::AppAudioEnded => true,
        }
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> &'static str {
        match self {
            AudioError::DeviceDisconnected => "Audio device was disconnected",
            AudioError::StreamFailed => "Audio stream encountered an error",
            AudioError::ProcessingFailed => "Audio processing failed",
            AudioError::ChannelClosed => "Audio channel was closed unexpectedly",
            AudioError::InitializationFailed => "Failed to initialize audio system",
            AudioError::ConfigurationError => "Audio configuration error",
            AudioError::PermissionDenied => "Microphone permission denied",
            AudioError::BufferOverflow => "Audio buffer overflow",
            AudioError::SampleRateUnsupported => "Audio sample rate not supported",
            AudioError::AppAudioEnded => "Google Chrome audio capture stopped (was Chrome closed?). The microphone is still recording.",
        }
    }
}

type ErrorCallback = Box<dyn Fn(&AudioError) + Send + Sync>;

/// Lock a mutex, recovering the data if a panicking thread poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Shared state of one recording session (capture callbacks, pipeline, commands).
#[derive(Default)]
pub struct RecordingState {
    is_recording: AtomicBool,
    is_paused: AtomicBool,
    /// Sender of the capture → pipeline channel; dropping it ends the pipeline.
    audio_sender: Mutex<Option<mpsc::UnboundedSender<AudioChunk>>>,
    error_count: AtomicU32,
    recoverable_error_count: AtomicU32,
    error_callback: Mutex<Option<ErrorCallback>>,
    recording_start: Mutex<Option<Instant>>,
    pause_start: Mutex<Option<Instant>>,
    total_pause_duration: Mutex<Duration>,
}

impl RecordingState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn start_recording(&self) -> Result<()> {
        self.is_recording.store(true, Ordering::SeqCst);
        *lock(&self.recording_start) = Some(Instant::now());
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Stop accepting audio and close the pipeline channel (the pipeline then drains).
    pub fn stop_recording(&self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.is_paused.store(false, Ordering::SeqCst);
        *lock(&self.pause_start) = None;
        *lock(&self.audio_sender) = None;
    }

    pub fn pause_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot pause when not recording"));
        }
        if self.is_paused() {
            return Err(anyhow::anyhow!("Recording is already paused"));
        }
        self.is_paused.store(true, Ordering::SeqCst);
        *lock(&self.pause_start) = Some(Instant::now());
        log::info!("Recording paused");
        Ok(())
    }

    pub fn resume_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot resume when not recording"));
        }
        if !self.is_paused() {
            return Err(anyhow::anyhow!("Recording is not paused"));
        }
        if let Some(pause_start) = lock(&self.pause_start).take() {
            let paused_for = pause_start.elapsed();
            *lock(&self.total_pause_duration) += paused_for;
            log::info!("Recording resumed after a {:.2}s pause", paused_for.as_secs_f64());
        }
        self.is_paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        self.is_recording() && !self.is_paused()
    }

    /// Attach the pipeline. Chunks captured before this are skipped, so this is audio time 0:
    /// the recording clock restarts here (opening the streams can take seconds).
    pub fn set_audio_sender(&self, sender: mpsc::UnboundedSender<AudioChunk>) {
        *lock(&self.audio_sender) = Some(sender);
        *lock(&self.recording_start) = Some(Instant::now());
    }

    /// Hand a capture chunk to the pipeline. Chunks are dropped while paused.
    pub fn send_audio_chunk(&self, chunk: AudioChunk) -> Result<()> {
        if self.is_paused() {
            return Ok(());
        }
        match lock(&self.audio_sender).as_ref() {
            Some(sender) => sender
                .send(chunk)
                .map_err(|_| anyhow::anyhow!("Failed to send audio chunk: channel closed")),
            None => Err(anyhow::anyhow!("Audio pipeline not ready - no sender available")),
        }
    }

    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&AudioError) + Send + Sync + 'static,
    {
        *lock(&self.error_callback) = Some(Box::new(callback));
    }

    /// Count an error, stop capture when errors pile up, and notify the callback.
    pub fn report_error(&self, error: AudioError) {
        let count = self.error_count.fetch_add(1, Ordering::SeqCst) + 1;
        if error.is_recoverable() {
            let recoverable = self.recoverable_error_count.fetch_add(1, Ordering::SeqCst) + 1;
            log::warn!("Recoverable audio error ({}): {:?}", recoverable, error);
            if recoverable >= 10 {
                log::error!("Too many recoverable errors ({}), stopping capture", recoverable);
                self.stop_recording();
            }
        } else {
            log::error!("Non-recoverable audio error: {:?}", error);
            self.stop_recording();
        }
        if let Some(callback) = lock(&self.error_callback).as_ref() {
            callback(&error);
        }
        if count >= 15 {
            log::error!("Too many audio errors ({}), stopping capture", count);
            self.stop_recording();
        }
    }

    /// Wall-clock time since start (pauses included).
    pub fn get_recording_duration(&self) -> Option<f64> {
        lock(&self.recording_start).map(|start| start.elapsed().as_secs_f64())
    }

    /// Recorded time excluding pauses.
    pub fn get_active_recording_duration(&self) -> Option<f64> {
        let start = (*lock(&self.recording_start))?;
        let current_pause = self.get_current_pause_duration().unwrap_or(0.0);
        Some(start.elapsed().as_secs_f64() - self.get_total_pause_duration() - current_pause)
    }

    pub fn get_total_pause_duration(&self) -> f64 {
        lock(&self.total_pause_duration).as_secs_f64()
    }

    pub fn get_current_pause_duration(&self) -> Option<f64> {
        if self.is_paused() {
            lock(&self.pause_start).map(|start| start.elapsed().as_secs_f64())
        } else {
            None
        }
    }

    /// Reset everything (end of session).
    pub fn cleanup(&self) {
        self.stop_recording();
        *lock(&self.error_callback) = None;
        *lock(&self.recording_start) = None;
        *lock(&self.total_pause_duration) = Duration::ZERO;
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk() -> AudioChunk {
        AudioChunk { data: vec![0.0; 4], device_type: DeviceType::Microphone }
    }

    #[test]
    fn pause_drops_chunks_and_stop_closes_the_channel() {
        let state = RecordingState::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.set_audio_sender(tx);
        state.start_recording().unwrap();

        state.send_audio_chunk(chunk()).unwrap();
        state.pause_recording().unwrap();
        assert!(state.pause_recording().is_err());
        state.send_audio_chunk(chunk()).unwrap();
        state.resume_recording().unwrap();
        state.send_audio_chunk(chunk()).unwrap();
        state.stop_recording();

        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, 2);
        assert!(rx.is_closed());
        assert!(state.send_audio_chunk(chunk()).is_err());
    }
}
