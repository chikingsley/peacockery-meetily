//! From aligned 48 kHz windows to 100 ms s16le 16 kHz frames, one bounded channel per live
//! session. Runs on the pipeline task: it never blocks; a full channel drops the frame.

use log::{info, warn};
use peacockery_voice::{Channel, FRAME_SAMPLES};
use rubato::{FftFixedInOut, Resampler};
use tokio::sync::mpsc;

use crate::audio::pipeline::PIPELINE_SAMPLE_RATE;

/// Frames buffered between the pipeline and a session task (5 s).
pub const TAP_CHANNEL_FRAMES: usize = 50;
/// Input block for the 48 → 16 kHz resampler (100 ms).
const RESAMPLER_INPUT: usize = 4800;

/// One 100 ms frame of a live session.
#[derive(Debug, Clone)]
pub struct LiveFrame {
    pub samples: Vec<i16>,
    /// Recording-clock seconds at the end of the window this frame came from.
    pub end: f64,
}

impl LiveFrame {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / peacockery_voice::SAMPLE_RATE as f64
    }
}

struct Tap {
    channel: Channel,
    resampler: Option<FftFixedInOut<f32>>,
    input: Vec<f32>,
    output: Vec<f32>,
    sender: mpsc::Sender<LiveFrame>,
    dropped: u64,
    last_end: f64,
}

impl Tap {
    fn push(&mut self, samples: &[f32], end: f64) {
        self.last_end = end;
        self.input.extend_from_slice(samples);
        match self.resampler.as_mut() {
            Some(resampler) => {
                while self.input.len() >= resampler.input_frames_next() {
                    let block: Vec<f32> = self.input.drain(..resampler.input_frames_next()).collect();
                    match resampler.process(&[block], None) {
                        Ok(mut out) => self.output.append(&mut out[0]),
                        Err(e) => {
                            warn!("Live resampler ({:?}) failed: {}", self.channel, e);
                            break;
                        }
                    }
                }
            }
            None => {
                // Decimate by 3 with a 3-tap average when rubato could not be built.
                let usable = self.input.len() / 3 * 3;
                self.output.extend(self.input.drain(..usable).collect::<Vec<_>>().chunks(3).map(|c| c.iter().sum::<f32>() / 3.0));
            }
        }
        while self.output.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = self.output.drain(..FRAME_SAMPLES).map(to_i16).collect();
            self.send(LiveFrame { samples: frame, end });
        }
    }

    fn send(&mut self, frame: LiveFrame) {
        match self.sender.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped += 1;
                if self.dropped == 1 || self.dropped % 50 == 0 {
                    warn!("Live {:?} session is behind: {} frames dropped so far", self.channel, self.dropped);
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// The pipeline side of the live sessions.
pub struct LiveTaps {
    taps: Vec<Tap>,
}

impl LiveTaps {
    pub(crate) fn new(senders: Vec<(Channel, mpsc::Sender<LiveFrame>)>) -> Self {
        let taps = senders
            .into_iter()
            .map(|(channel, sender)| {
                let resampler = FftFixedInOut::<f32>::new(PIPELINE_SAMPLE_RATE as usize, peacockery_voice::SAMPLE_RATE as usize, RESAMPLER_INPUT, 1)
                    .map_err(|e| warn!("Live resampler unavailable, using plain decimation: {}", e))
                    .ok();
                Tap { channel, resampler, input: Vec::with_capacity(RESAMPLER_INPUT * 2), output: Vec::with_capacity(FRAME_SAMPLES * 2), sender, dropped: 0, last_end: 0.0 }
            })
            .collect();
        Self { taps }
    }

    /// One aligned 48 kHz window; `end` is its recording-clock end time.
    pub fn push(&mut self, mic: &[f32], sys: &[f32], mixed: &[f32], end: f64) {
        for tap in &mut self.taps {
            let samples = match tap.channel {
                Channel::Mic => mic,
                Channel::System => sys,
                Channel::Mixed => mixed,
            };
            tap.push(samples, end);
        }
    }

    /// End of audio: flush what the resamplers hold and close every session's input.
    pub fn close(mut self) {
        for tap in &mut self.taps {
            if let Some(resampler) = tap.resampler.as_ref() {
                let pad = resampler.input_frames_next().saturating_sub(tap.input.len() % resampler.input_frames_next());
                if !tap.input.is_empty() {
                    let end = tap.last_end;
                    tap.push(&vec![0.0; pad], end);
                }
            }
            if tap.dropped > 0 {
                info!("Live {:?} tap closed; {} frames dropped in total", tap.channel, tap.dropped);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_become_100_ms_16k_frames() {
        let (tx, mut rx) = mpsc::channel(TAP_CHANNEL_FRAMES);
        let mut taps = LiveTaps::new(vec![(Channel::Mic, tx)]);
        let tone: Vec<f32> = (0..4800).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5).collect();
        for i in 0..10 {
            taps.push(&tone, &[0.0; 4800], &tone, (i + 1) as f64 * 0.1);
        }
        let mut frames = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            frames.push(frame);
        }
        assert!(frames.len() >= 9, "got {} frames", frames.len());
        assert!(frames.iter().all(|f| f.samples.len() == FRAME_SAMPLES));
        let peak = frames.last().unwrap().samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 12000 && peak < 20000, "a 0.5 tone keeps its level, peak {}", peak);
        assert!(frames.windows(2).all(|w| w[0].end <= w[1].end));
    }

    #[test]
    fn a_full_channel_drops_instead_of_blocking() {
        let (tx, _rx) = mpsc::channel(2);
        let mut taps = LiveTaps::new(vec![(Channel::System, tx)]);
        for i in 0..20 {
            taps.push(&[0.0; 4800], &[0.1; 4800], &[0.1; 4800], i as f64 * 0.1);
        }
        assert!(taps.taps[0].dropped >= 15);
    }
}
