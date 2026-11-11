//! Align → mix → record, plus the live, energy and level taps (ARCHITECTURE.md §4).
//!
//! Capture chunks (mono 48 kHz, see `capture.rs`) arrive from two independent streams.
//! `AudioAligner` turns them into equal 100 ms windows on the mic clock; every window then
//! goes to the recording saver (the mix), the live sessions, the energy track and the
//! `audio-levels` meter.

use log::{error, info, warn};
use peacockery_voice::{rms_dbfs, EnergyTrack};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::levels::{LevelMeter, LevelsCallback};
use super::recording_state::{AudioChunk, DeviceType, RecordingState};
use crate::final_transcript::ENERGY_FILE;
use crate::live::LiveTaps;

/// Sample rate of every chunk leaving `AudioCapture`.
pub const PIPELINE_SAMPLE_RATE: u32 = 48000;
/// Aligned window length: one live frame, one energy frame, one level update.
pub const WINDOW_MS: u32 = 100;

/// A stream with no samples for this long is idle (idle WASAPI loopback delivers no callbacks).
const IDLE_SECONDS: f64 = 0.25;
/// Mic audio waiting for slightly late system audio beyond one window; past this the
/// system shortfall is zero-filled.
const MAX_WAIT_SECONDS: f64 = 0.3;
/// System audio buffered ahead of the mic beyond this is dropped (oldest first) …
const MAX_SYS_BACKLOG_SECONDS: f64 = 1.2;
/// … down to this.
const RESYNC_SYS_BACKLOG_SECONDS: f64 = 0.2;
/// Safety cap on the mic buffer (only reached if the pipeline stalls).
const MAX_MIC_BACKLOG_SECONDS: f64 = 5.0;
/// How often the pipeline re-checks idle streams when no chunk arrives.
const IDLE_CHECK_INTERVAL: Duration = Duration::from_millis(50);
/// Energy frames between crash-safety flushes of `channel-energy.json` (30 s).
const ENERGY_FLUSH_FRAMES: usize = 300;

/// Turns the two asynchronous capture streams into equal windows.
///
/// The mic is the master clock: each window takes exactly one window of mic audio and the
/// same number of system samples. System audio is zero-filled only while its stream is idle
/// (or after a bounded wait), and trimmed when it runs more than 1.2 s ahead. When the mic
/// itself is idle (system-only recording, mic hot-swap) the system stream drives the clock.
/// Times are seconds on any monotonic clock, passed in so the logic is testable.
pub struct AudioAligner {
    mic: VecDeque<f32>,
    sys: VecDeque<f32>,
    window: usize,
    max_wait: usize,
    max_sys: usize,
    resync_sys: usize,
    max_mic: usize,
    started: f64,
    mic_last: Option<f64>,
    sys_last: Option<f64>,
    sys_dropped: u64,
    sys_zero_filled: u64,
}

impl AudioAligner {
    pub fn new(sample_rate: u32, now: f64) -> Self {
        let samples = |seconds: f64| (sample_rate as f64 * seconds) as usize;
        let window = (sample_rate * WINDOW_MS / 1000) as usize;
        Self {
            mic: VecDeque::with_capacity(window * 4),
            sys: VecDeque::with_capacity(window * 4),
            window,
            max_wait: samples(MAX_WAIT_SECONDS),
            max_sys: samples(MAX_SYS_BACKLOG_SECONDS),
            resync_sys: samples(RESYNC_SYS_BACKLOG_SECONDS),
            max_mic: samples(MAX_MIC_BACKLOG_SECONDS),
            started: now,
            mic_last: None,
            sys_last: None,
            sys_dropped: 0,
            sys_zero_filled: 0,
        }
    }



    pub fn add(&mut self, device_type: &DeviceType, samples: &[f32], now: f64) {
        match device_type {
            DeviceType::Microphone => {
                self.mic.extend(samples);
                self.mic_last = Some(now);
                if self.mic.len() > self.max_mic {
                    let excess = self.mic.len() - self.max_mic;
                    warn!("Mic buffer overflow, dropping the oldest {} samples", excess);
                    self.mic.drain(..excess);
                }
            }
            DeviceType::System => {
                self.sys.extend(samples);
                self.sys_last = Some(now);
                if self.sys.len() > self.max_sys {
                    let excess = self.sys.len() - self.resync_sys;
                    self.sys_dropped += excess as u64;
                    warn!(
                        "System audio ran {:.2} s ahead of the mic; dropped the oldest {} samples to resync ({} dropped so far)",
                        self.sys.len() as f64 / (self.window as f64 * 10.0),
                        excess,
                        self.sys_dropped
                    );
                    self.sys.drain(..excess);
                }
            }
        }
    }

    fn idle(&self, last: Option<f64>, now: f64) -> bool {
        now - last.unwrap_or(self.started) > IDLE_SECONDS
    }

    fn take(buffer: &mut VecDeque<f32>, size: usize) -> (Vec<f32>, usize) {
        let take = buffer.len().min(size);
        let mut window: Vec<f32> = buffer.drain(..take).collect();
        window.resize(size, 0.0);
        (window, size - take)
    }

    /// The next aligned `(mic, sys)` window, or `None` when it is not ready yet.
    pub fn next_window(&mut self, now: f64) -> Option<(Vec<f32>, Vec<f32>)> {
        let size = self.window;
        if self.mic.len() >= size {
            let sys_ready = self.sys.len() >= size;
            let stop_waiting = self.idle(self.sys_last, now) || self.mic.len() >= size + self.max_wait;
            if !sys_ready && !stop_waiting {
                return None;
            }
            let (mic, _) = Self::take(&mut self.mic, size);
            let (sys, filled) = Self::take(&mut self.sys, size);
            self.sys_zero_filled += filled as u64;
            return Some((mic, sys));
        }
        if self.sys.len() >= size && self.idle(self.mic_last, now) {
            let (mic, _) = Self::take(&mut self.mic, size);
            let (sys, _) = Self::take(&mut self.sys, size);
            return Some((mic, sys));
        }
        None
    }

    /// Whatever is left at stop, padded to a common length.
    pub fn drain_remaining(&mut self) -> Option<(Vec<f32>, Vec<f32>)> {
        let len = self.mic.len().max(self.sys.len());
        if len == 0 {
            return None;
        }
        Some((Self::take(&mut self.mic, len).0, Self::take(&mut self.sys, len).0))
    }

    /// (system samples dropped to resync, system samples zero-filled)
    pub fn stats(&self) -> (u64, u64) {
        (self.sys_dropped, self.sys_zero_filled)
    }
}

/// `mic + sys`, scaled down proportionally when the sum would clip.
fn mix_window(mic_window: &[f32], sys_window: &[f32]) -> Vec<f32> {
    let len = mic_window.len().max(sys_window.len());
    (0..len)
        .map(|i| {
            let sum = mic_window.get(i).copied().unwrap_or(0.0) + sys_window.get(i).copied().unwrap_or(0.0);
            if sum.abs() > 1.0 {
                sum / sum.abs()
            } else {
                sum
            }
        })
        .collect()
}

/// `channel-energy.json` of the meeting, flushed every 30 s and at the end.
pub struct EnergyRecorder {
    path: PathBuf,
    track: EnergyTrack,
    since_flush: usize,
}

impl EnergyRecorder {
    pub fn new(folder: PathBuf) -> Self {
        Self { path: folder.join(ENERGY_FILE), track: EnergyTrack::new(WINDOW_MS), since_flush: 0 }
    }

    fn push(&mut self, mic_db: f32, sys_db: f32) {
        self.track.push_db(mic_db, sys_db);
        self.since_flush += 1;
        if self.since_flush >= ENERGY_FLUSH_FRAMES {
            self.since_flush = 0;
            let path = self.path.clone();
            let track = self.track.clone();
            tokio::task::spawn_blocking(move || write_energy(&path, &track));
        }
    }

    async fn finish(self) {
        let EnergyRecorder { path, track, .. } = self;
        if let Err(e) = tokio::task::spawn_blocking(move || write_energy(&path, &track)).await {
            error!("Energy track write task failed: {}", e);
        }
    }
}

fn write_energy(path: &std::path::Path, track: &EnergyTrack) {
    let result = serde_json::to_vec(track)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, path)
        });
    if let Err(e) = result {
        warn!("Failed to write {}: {}", path.display(), e);
    }
}

/// What one capture stream delivered to the pipeline (mono 48 kHz, before alignment).
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct SourceStats {
    pub chunks: u64,
    pub samples: u64,
    pub peak: f32,
    pub rms: f32,
    #[serde(skip)]
    sum_squares: f64,
}

impl SourceStats {
    fn add(&mut self, samples: &[f32]) {
        self.chunks += 1;
        self.samples += samples.len() as u64;
        for &sample in samples {
            self.peak = self.peak.max(sample.abs());
            self.sum_squares += f64::from(sample) * f64::from(sample);
        }
        self.rms = (self.sum_squares / self.samples.max(1) as f64).sqrt() as f32;
    }
}

/// Totals of a finished pipeline run.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct PipelineStats {
    pub mic: SourceStats,
    pub system: SourceStats,
    /// Samples written to the recording (aligned windows).
    pub aligned_samples: u64,
    /// System samples dropped because the stream ran ahead of the mic.
    pub system_dropped: u64,
    /// System samples zero-filled because the stream was idle or late.
    pub system_zero_filled: u64,
}

/// Where aligned windows go.
#[derive(Default)]
pub struct PipelineOutputs {
    /// The mix, to the recording saver.
    pub recording: Option<mpsc::UnboundedSender<AudioChunk>>,
    pub live: Option<LiveTaps>,
    pub energy: Option<EnergyRecorder>,
    pub levels: Option<LevelsCallback>,
}

/// Aligns mic and system audio and feeds every output.
pub struct AudioPipeline {
    receiver: mpsc::UnboundedReceiver<AudioChunk>,
    sample_rate: u32,
    samples_out: u64,
    mic_stats: SourceStats,
    system_stats: SourceStats,
    aligner: AudioAligner,
    outputs: PipelineOutputs,
    meter: LevelMeter,
    clock: Instant,
}

impl AudioPipeline {
    pub fn new(receiver: mpsc::UnboundedReceiver<AudioChunk>, sample_rate: u32, outputs: PipelineOutputs) -> Self {
        Self {
            receiver,
            sample_rate,
            samples_out: 0,
            mic_stats: SourceStats::default(),
            system_stats: SourceStats::default(),
            aligner: AudioAligner::new(sample_rate, 0.0),
            outputs,
            meter: LevelMeter::default(),
            clock: Instant::now(),
        }
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    /// Runs until every sender of the capture channel is dropped, then drains the tail and
    /// closes every output (the saver's channel and the live taps) and writes the energy track.
    pub async fn run(mut self) -> PipelineStats {
        info!("Audio pipeline started");
        let mut idle_check = tokio::time::interval(IDLE_CHECK_INTERVAL);
        idle_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                chunk = self.receiver.recv() => match chunk {
                    Some(chunk) => {
                        match chunk.device_type {
                            DeviceType::Microphone => self.mic_stats.add(&chunk.data),
                            DeviceType::System => self.system_stats.add(&chunk.data),
                        }
                        let now = self.now();
                        self.aligner.add(&chunk.device_type, &chunk.data, now);
                    }
                    None => break,
                },
                _ = idle_check.tick() => {}
            }
            let now = self.now();
            while let Some((mic, sys)) = self.aligner.next_window(now) {
                self.process_window(&mic, &sys);
            }
        }
        if let Some((mic, sys)) = self.aligner.drain_remaining() {
            self.process_window(&mic, &sys);
        }
        let (system_dropped, system_zero_filled) = self.aligner.stats();
        let stats = PipelineStats {
            mic: self.mic_stats,
            system: self.system_stats,
            aligned_samples: self.samples_out,
            system_dropped,
            system_zero_filled,
        };
        info!(
            "Audio pipeline ended: {:.1} s aligned; mic {} chunks, {} samples, peak {:.3}; system {} chunks, {} samples, peak {:.3}, {} dropped to resync, {} zero-filled",
            stats.aligned_samples as f64 / self.sample_rate as f64,
            stats.mic.chunks,
            stats.mic.samples,
            stats.mic.peak,
            stats.system.chunks,
            stats.system.samples,
            stats.system.peak,
            stats.system_dropped,
            stats.system_zero_filled
        );
        let PipelineOutputs { recording, live, energy, .. } = self.outputs;
        drop(recording);
        if let Some(live) = live {
            live.close();
        }
        if let Some(energy) = energy {
            energy.finish().await;
        }
        stats
    }

    /// One aligned window: `mic` and `sys` are equal-length mono 48 kHz.
    fn process_window(&mut self, mic: &[f32], sys: &[f32]) {
        let mixed = mix_window(mic, sys);
        let start = self.samples_out as f64 / self.sample_rate as f64;
        self.samples_out += mic.len() as u64;
        let end = self.samples_out as f64 / self.sample_rate as f64;

        if let Some(live) = self.outputs.live.as_mut() {
            live.push(mic, sys, &mixed, end);
        }
        let mic_db = rms_dbfs(mic);
        let sys_db = rms_dbfs(sys);
        if let Some(energy) = self.outputs.energy.as_mut() {
            energy.push(mic_db, sys_db);
        }
        if let Some(levels) = self.outputs.levels.as_mut() {
            levels(self.meter.frame(mic_db, sys_db, end - start));
        }
        if let Some(sender) = &self.outputs.recording {
            let recording_chunk = AudioChunk { data: mixed, device_type: DeviceType::Microphone };
            if sender.send(recording_chunk).is_err() {
                warn!("Recording saver channel closed; mixed audio dropped");
            }
        }
    }
}

/// Owns the pipeline task and its input channel.
#[derive(Default)]
pub struct AudioPipelineManager {
    pipeline_handle: Option<JoinHandle<PipelineStats>>,
}

impl AudioPipelineManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the pipeline. The only sender of its input channel is handed to
    /// `RecordingState`, so `RecordingState::stop_recording` closes it.
    pub fn start(&mut self, state: &RecordingState, outputs: PipelineOutputs) {
        let (audio_sender, audio_receiver) = mpsc::unbounded_channel::<AudioChunk>();
        let pipeline = AudioPipeline::new(audio_receiver, PIPELINE_SAMPLE_RATE, outputs);
        state.set_audio_sender(audio_sender);
        self.pipeline_handle = Some(tokio::spawn(pipeline.run()));
        info!("Audio pipeline manager started");
    }

    /// Wait for the pipeline to drain and return its totals. The caller must already have
    /// closed the input channel (`RecordingState::stop_recording`).
    pub async fn wait_for_drain(&mut self) -> Option<PipelineStats> {
        match self.pipeline_handle.take()?.await {
            Ok(stats) => Some(stats),
            Err(e) => {
                error!("Pipeline task failed: {}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000; // 100-sample windows, 1 sample = 1 ms

    fn mic(a: &mut AudioAligner, n: usize, now: f64) {
        a.add(&DeviceType::Microphone, &vec![0.5; n], now);
    }

    fn sys(a: &mut AudioAligner, n: usize, now: f64) {
        a.add(&DeviceType::System, &vec![0.25; n], now);
    }

    fn drain(a: &mut AudioAligner, now: f64) -> Vec<(Vec<f32>, Vec<f32>)> {
        std::iter::from_fn(|| a.next_window(now)).collect()
    }

    #[test]
    fn mix_window_scales_instead_of_clipping() {
        let mixed = mix_window(&[0.8, -0.8, 0.1], &[0.8, -0.8, 0.1]);
        assert_eq!(mixed, vec![1.0, -1.0, 0.2]);
    }

    #[test]
    fn bursty_system_audio_is_waited_for_not_zero_filled() {
        let mut a = AudioAligner::new(RATE, 0.0);
        // Mic arrives every 10 ms; system arrives in 40 ms bursts.
        let mut windows = Vec::new();
        for ms in (10..=1000).step_by(10) {
            let now = ms as f64 / 1000.0;
            mic(&mut a, 10, now);
            if ms % 40 == 0 {
                sys(&mut a, 40, now);
            }
            windows.extend(drain(&mut a, now));
        }
        assert!(windows.len() >= 9, "got {} windows", windows.len());
        for (m, s) in &windows {
            assert_eq!(m.len(), 100);
            assert_eq!(s.len(), 100);
            assert!(s.iter().all(|&x| x == 0.25), "system audio was zero-filled");
        }
        assert_eq!(a.stats(), (0, 0));
    }

    #[test]
    fn idle_system_stream_is_zero_filled_after_250_ms() {
        let mut a = AudioAligner::new(RATE, 0.0);
        mic(&mut a, 100, 0.1);
        assert!(a.next_window(0.1).is_none(), "waits for the system stream at first");
        let (m, s) = a.next_window(0.3).expect("system idle > 250 ms");
        assert_eq!(m, vec![0.5; 100]);
        assert_eq!(s, vec![0.0; 100]);

        // System comes back: its samples are used again.
        sys(&mut a, 100, 0.35);
        mic(&mut a, 100, 0.4);
        let (_, s) = a.next_window(0.4).unwrap();
        assert_eq!(s, vec![0.25; 100]);

        // System goes quiet again: partial system data is padded once idle.
        sys(&mut a, 30, 0.45);
        mic(&mut a, 100, 0.5);
        assert!(a.next_window(0.5).is_none(), "system is not idle yet");
        let (_, s) = a.next_window(0.75).unwrap();
        assert_eq!(&s[..30], &[0.25; 30]);
        assert!(s[30..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn slow_system_waits_only_a_bounded_time() {
        let mut a = AudioAligner::new(RATE, 0.0);
        sys(&mut a, 50, 0.0);
        // System keeps arriving (never idle) but too little; mic piles up.
        mic(&mut a, 300, 0.1);
        sys(&mut a, 1, 0.1);
        assert!(a.next_window(0.1).is_none(), "300 mic samples are within the wait bound");
        mic(&mut a, 200, 0.2);
        sys(&mut a, 1, 0.2);
        let (_, s) = a.next_window(0.2).expect("wait bound exceeded");
        assert_eq!(s.iter().filter(|&&x| x == 0.25).count(), 52);
    }

    #[test]
    fn fast_system_is_trimmed_to_resync() {
        let mut a = AudioAligner::new(RATE, 0.0);
        // System drifts ahead: 1300 samples buffered vs mic.
        sys(&mut a, 1300, 0.1);
        let (dropped, _) = a.stats();
        assert_eq!(dropped, 1100, "trimmed down to 200 ms");
        mic(&mut a, 100, 0.1);
        assert!(a.next_window(0.1).is_some());
        mic(&mut a, 100, 0.2);
        assert!(a.next_window(0.2).is_some());
        mic(&mut a, 100, 0.4);
        let (_, s) = a.next_window(0.4).expect("system idle");
        assert_eq!(s, vec![0.0; 100], "only 200 ms of system audio were kept");
    }

    #[test]
    fn idle_mic_lets_system_drive_the_clock() {
        let mut a = AudioAligner::new(RATE, 0.0);
        sys(&mut a, 100, 0.1);
        assert!(a.next_window(0.1).is_none());
        let (m, s) = a.next_window(0.3).expect("mic idle > 250 ms");
        assert_eq!(m, vec![0.0; 100]);
        assert_eq!(s, vec![0.25; 100]);
    }

    #[test]
    fn drains_the_tail_padded() {
        let mut a = AudioAligner::new(RATE, 0.0);
        mic(&mut a, 70, 0.0);
        sys(&mut a, 20, 0.0);
        let (m, s) = a.drain_remaining().unwrap();
        assert_eq!((m.len(), s.len()), (70, 70));
        assert!(a.drain_remaining().is_none());
    }
}
