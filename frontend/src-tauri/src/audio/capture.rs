//! `AudioCapture` runs on the capture callback thread (cpal or the Chrome process loopback):
//! mono, resample to 48 kHz, mic-only high-pass + EBU R128, then hands an `AudioChunk` to
//! the pipeline channel.

use log::{debug, error, info, warn};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::audio_processing::{audio_to_mono, HighPassFilter, LoudnessNormalizer};
use super::devices::AudioDevice;
use super::pipeline::PIPELINE_SAMPLE_RATE;
use super::recording_state::{AudioChunk, AudioError, DeviceType, RecordingState};

/// Fixed input block for the persistent capture resampler.
const RESAMPLER_CHUNK_SIZE: usize = 512;

fn build_resampler(from_rate: u32) -> Option<SincFixedIn<f32>> {
    let ratio = PIPELINE_SAMPLE_RATE as f64 / from_rate as f64;
    let (sinc_len, interpolation, oversampling_factor) = if ratio >= 2.0 || ratio <= 0.5 {
        (512, SincInterpolationType::Cubic, 512)
    } else if ratio >= 1.5 {
        (384, SincInterpolationType::Cubic, 384)
    } else if ratio > 1.0 {
        (256, SincInterpolationType::Linear, 256)
    } else {
        (384, SincInterpolationType::Linear, 384)
    };
    let params = SincInterpolationParameters {
        sinc_len,
        f_cutoff: 0.95,
        interpolation,
        oversampling_factor,
        window: WindowFunction::BlackmanHarris2,
    };
    match SincFixedIn::<f32>::new(ratio, 2.0, params, RESAMPLER_CHUNK_SIZE, 1) {
        Ok(resampler) => Some(resampler),
        Err(e) => {
            warn!(
                "Failed to create persistent resampler ({} Hz): {}",
                from_rate, e
            );
            None
        }
    }
}

/// Compare the first sample's device time with the end of the previous packet.
#[derive(Clone, Default)]
struct PacketClock {
    epoch: u32,
    next: Duration,
}

impl PacketClock {
    fn observe(&mut self, epoch: u32, start: Duration, length: Duration) -> Option<Duration> {
        let gap = start.saturating_sub(self.next);
        let same_sequence = self.epoch == epoch && epoch != 0;
        self.epoch = epoch;
        let packet_end = start + length;
        self.next = if same_sequence {
            self.next.max(packet_end)
        } else {
            packet_end
        };
        // Ignore small timestamp jitter. Each packet becomes the next baseline,
        // so a reported discontinuity counts once.
        (same_sequence && gap >= Duration::from_millis(250)).then_some(gap)
    }
}

/// Per-device capture processor, called from the cpal callback.
#[derive(Clone)]
pub struct AudioCapture {
    clock: PacketClock,
    clock_origin: Option<cpal::StreamInstant>,
    device: Arc<AudioDevice>,
    state: Arc<RecordingState>,
    sample_rate: u32,
    channels: u16,
    chunk_counter: Arc<AtomicU64>,
    device_type: DeviceType,
    needs_resampling: bool,
    /// Persistent resampler (one per stream) so energy is preserved across chunks.
    resampler: Arc<Mutex<Option<SincFixedIn<f32>>>>,
    resampler_input_buffer: Arc<Mutex<Vec<f32>>>,
    /// Microphone only.
    high_pass_filter: Arc<Mutex<Option<HighPassFilter>>>,
    /// Microphone only: EBU R128 to -23 LUFS.
    normalizer: Arc<Mutex<Option<LoudnessNormalizer>>>,
}

impl AudioCapture {
    pub fn new(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        sample_rate: u32,
        channels: u16,
        device_type: DeviceType,
    ) -> Self {
        let needs_resampling = sample_rate != PIPELINE_SAMPLE_RATE;
        info!(
            "[{:?}] '{}' captures at {} Hz, {} ch{}",
            device_type,
            device.name,
            sample_rate,
            channels,
            if needs_resampling {
                " (resampling to 48 kHz)"
            } else {
                ""
            }
        );

        let (high_pass_filter, normalizer) = if device_type == DeviceType::Microphone {
            let normalizer = LoudnessNormalizer::new(1, PIPELINE_SAMPLE_RATE)
                .map_err(|e| warn!("Mic loudness normalizer disabled: {}", e))
                .ok();
            (
                Some(HighPassFilter::new(PIPELINE_SAMPLE_RATE, 80.0)),
                normalizer,
            )
        } else {
            (None, None)
        };

        let resampler = if needs_resampling {
            build_resampler(sample_rate)
        } else {
            None
        };

        Self {
            clock: PacketClock::default(),
            clock_origin: None,
            device,
            state,
            sample_rate,
            channels,
            chunk_counter: Arc::new(AtomicU64::new(0)),
            device_type,
            needs_resampling,
            resampler: Arc::new(Mutex::new(resampler)),
            resampler_input_buffer: Arc::new(Mutex::new(Vec::with_capacity(
                RESAMPLER_CHUNK_SIZE * 2,
            ))),
            high_pass_filter: Arc::new(Mutex::new(high_pass_filter)),
            normalizer: Arc::new(Mutex::new(normalizer)),
        }
    }

    /// Resample to 48 kHz through the persistent resampler. Returns `None` while the
    /// input buffer is still filling a 512-sample block (the audio is kept, not lost).
    fn resample_to_pipeline_rate(&self, mono: Vec<f32>) -> Option<Vec<f32>> {
        let (Ok(mut buffer), Ok(mut resampler_lock)) =
            (self.resampler_input_buffer.lock(), self.resampler.lock())
        else {
            return Some(super::audio_processing::resample_audio(
                &mono,
                self.sample_rate,
                PIPELINE_SAMPLE_RATE,
            ));
        };
        let Some(resampler) = resampler_lock.as_mut() else {
            return Some(super::audio_processing::resample_audio(
                &mono,
                self.sample_rate,
                PIPELINE_SAMPLE_RATE,
            ));
        };

        buffer.extend_from_slice(&mono);
        let mut output = Vec::new();
        while buffer.len() >= RESAMPLER_CHUNK_SIZE {
            let block: Vec<f32> = buffer.drain(..RESAMPLER_CHUNK_SIZE).collect();
            match resampler.process(&[block], None) {
                Ok(mut waves_out) => {
                    if let Some(channel) = waves_out.pop() {
                        output.extend_from_slice(&channel);
                    }
                }
                Err(e) => {
                    warn!("Persistent resampler failed: {}", e);
                    break;
                }
            }
        }
        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }

    /// A microphone delivers continuously. Render loopback can omit silent packets by design.
    pub fn process_device_packet(&mut self, data: &[f32], info: &cpal::InputCallbackInfo) {
        if self.device_type == DeviceType::Microphone && !data.is_empty() {
            self.observe_mic_packet(data.len() / usize::from(self.channels), info.timestamp());
        }
        self.process_audio_data(data);
    }

    fn observe_mic_packet(&mut self, frames: usize, timestamp: cpal::InputStreamTimestamp) {
        let origin = self.clock_origin.get_or_insert(timestamp.capture);
        let Some(start) = timestamp.capture.duration_since(origin) else {
            self.clock_origin = None;
            self.clock = PacketClock::default();
            return;
        };
        let length = Duration::from_secs_f64(frames as f64 / f64::from(self.sample_rate));
        let Some(gap) = self
            .clock
            .observe(self.state.capture_epoch(), start, length)
        else {
            return;
        };
        let latency = timestamp
            .callback
            .duration_since(&timestamp.capture)
            .unwrap_or_default();
        let end = self
            .state
            .get_active_recording_duration()
            .unwrap_or_default()
            - latency.as_secs_f64();
        // A driver clock jump can exceed the entire recording. Only report intervals
        // that fit inside active capture, allowing a packet of timing uncertainty.
        if gap.as_secs_f64() <= end + length.as_secs_f64() {
            self.state.record_mic_gap(gap);
            warn!("Microphone capture gap: device={} duration={:.3}s estimated recording interval={:.3}..{:.3}s", self.device.name, gap.as_secs_f64(), (end - gap.as_secs_f64()).max(0.0), end);
        }
    }

    /// Process one cpal callback buffer.
    pub fn process_audio_data(&self, data: &[f32]) {
        if !self.state.is_recording() {
            return;
        }

        let mono = if self.channels > 1 {
            audio_to_mono(data, self.channels)
        } else {
            data.to_vec()
        };

        let mut mono = if self.needs_resampling {
            match self.resample_to_pipeline_rate(mono) {
                Some(resampled) => resampled,
                None => return,
            }
        } else {
            mono
        };

        if self.device_type == DeviceType::Microphone {
            if let Ok(mut hpf) = self.high_pass_filter.lock() {
                if let Some(filter) = hpf.as_mut() {
                    mono = filter.process(&mono);
                }
            }
            if let Ok(mut normalizer) = self.normalizer.lock() {
                if let Some(normalizer) = normalizer.as_mut() {
                    mono = normalizer.normalize_loudness(&mono);
                }
            }
        }

        let chunk_id = self.chunk_counter.fetch_add(1, Ordering::SeqCst);
        let audio_chunk = AudioChunk {
            data: mono,
            device_type: self.device_type.clone(),
        };

        if let Err(e) = self.state.send_audio_chunk(audio_chunk) {
            let message = e.to_string();
            if message.contains("Audio pipeline not ready") {
                debug!("Audio pipeline not ready yet, skipping chunk {}", chunk_id);
                return;
            }
            warn!("Failed to send audio chunk: {}", message);
            let error = if message.contains("channel closed") {
                AudioError::ChannelClosed
            } else {
                AudioError::ProcessingFailed
            };
            self.state.report_error(error);
        }
    }

    /// Map a cpal stream error onto `AudioError` and report it.
    pub fn handle_stream_error(&self, error: cpal::StreamError) {
        error!("Audio stream error for {}: {}", self.device.name, error);
        let error_str = error.to_string().to_lowercase();
        let audio_error = if [
            "device is no longer available",
            "device not found",
            "device disconnected",
            "no such device",
            "device unavailable",
            "device removed",
        ]
        .iter()
        .any(|needle| error_str.contains(needle))
        {
            warn!("Device disconnect detected for: {}", self.device.name);
            AudioError::DeviceDisconnected
        } else if error_str.contains("permission") || error_str.contains("access denied") {
            AudioError::PermissionDenied
        } else if error_str.contains("channel closed") {
            AudioError::ChannelClosed
        } else {
            AudioError::StreamFailed
        };
        self.state.report_error(audio_error);
    }

    /// A non-cpal source (the Chrome process loopback) stopped delivering audio.
    pub fn report_source_ended(&self, message: &str) {
        error!("Audio source '{}' ended: {}", self.device.name, message);
        self.state.report_error(AudioError::AppAudioEnded);
    }
}

#[cfg(test)]
mod packet_clock_tests {
    use super::*;

    #[test]
    fn counts_a_hole_once_and_accepts_delayed_or_coalesced_packets() {
        let mut clock = PacketClock::default();
        let ms = Duration::from_millis;
        // Identical hardware timeline for immediate or delayed delivery.
        assert_eq!(clock.observe(1, ms(0), ms(10)), None);
        assert_eq!(clock.observe(1, ms(10), ms(750)), None);
        assert_eq!(clock.observe(1, ms(760), ms(10)), None);
        assert_eq!(clock.observe(1, ms(1770), ms(10)), Some(ms(1000)));
        assert_eq!(clock.observe(1, ms(1780), ms(10)), None);
        assert_eq!(clock.observe(1, ms(1791), ms(10)), None);
        assert_eq!(clock.observe(1, ms(770), ms(10)), None);
        assert_eq!(clock.observe(1, ms(1801), ms(10)), None);
    }

    #[test]
    fn prestart_pause_resume_and_device_swap_start_new_sequences() {
        let mut clock = PacketClock::default();
        let ms = Duration::from_millis;
        assert_eq!(clock.observe(0, ms(0), ms(10)), None);
        assert_eq!(clock.observe(1, ms(950), ms(10)), None);
        assert_eq!(clock.observe(0, ms(960), ms(10)), None);
        assert_eq!(clock.observe(2, ms(2900), ms(10)), None);
        // A pause/resume may deliver no callbacks at all.
        assert_eq!(clock.observe(3, ms(4500), ms(10)), None);
        let mut replacement = PacketClock::default();
        assert_eq!(replacement.observe(3, ms(9000), ms(10)), None);
    }
}
