//! Channel energy track and speaker naming for the final transcript (ARCHITECTURE.md §6.3).

use serde::{Deserialize, Serialize};

use crate::turns::Turn;

/// System frames at or above this level (dBFS) count as remote audio.
pub const SYS_ACTIVE_DBFS: f32 = -50.0;
/// … and only when the system channel is no more than this far below the mic (dB).
pub const SYS_MIC_MARGIN_DB: f32 = 6.0;
/// A speaker whose system-active fraction is below this (and is the lowest) is the local user.
pub const ME_MAX_SYSTEM_FRACTION: f64 = 0.35;
/// Level written for digital silence (JSON cannot hold -inf).
pub const SILENCE_DBFS: f32 = -100.0;
/// Frame length the app writes.
pub const DEFAULT_FRAME_MS: u32 = 100;

/// `channel-energy.json`: per-frame RMS dBFS of the mic and system channels on the recording clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnergyTrack {
    pub frame_ms: u32,
    pub mic_db: Vec<f32>,
    pub sys_db: Vec<f32>,
}

impl Default for EnergyTrack {
    fn default() -> Self {
        Self::new(DEFAULT_FRAME_MS)
    }
}

impl EnergyTrack {
    pub fn new(frame_ms: u32) -> Self {
        Self {
            frame_ms: frame_ms.max(1),
            mic_db: Vec::new(),
            sys_db: Vec::new(),
        }
    }



    /// Append one frame of already computed levels.
    pub fn push_db(&mut self, mic_db: f32, sys_db: f32) {
        self.mic_db.push(clamp_db(mic_db));
        self.sys_db.push(clamp_db(sys_db));
    }

    /// Number of frames present on both channels.
    pub fn len(&self) -> usize {
        self.mic_db.len().min(self.sys_db.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remote audio is present in frame `i`: sys ≥ −50 dBFS and sys ≥ mic − 6 dB.
    pub fn system_active(&self, i: usize) -> bool {
        let (Some(&sys), Some(&mic)) = (self.sys_db.get(i), self.mic_db.get(i)) else {
            return false;
        };
        sys >= SYS_ACTIVE_DBFS && sys >= mic - SYS_MIC_MARGIN_DB
    }

    /// True when any frame carries system audio above the activity floor.
    pub fn has_system_audio(&self) -> bool {
        self.sys_db
            .iter()
            .take(self.len())
            .any(|&db| db >= SYS_ACTIVE_DBFS)
    }

    /// Frame index range covering `[start, end)` seconds, clamped to the track.
    fn frames(&self, start: f64, end: f64) -> std::ops::Range<usize> {
        let per_second = 1000.0 / self.frame_ms as f64;
        let len = self.len();
        let first = ((start.max(0.0) * per_second).floor() as usize).min(len);
        let last = ((end.max(start).max(0.0) * per_second).ceil() as usize).min(len);
        if last <= first && first < len {
            return first..first + 1;
        }
        first..last
    }

    /// (system-active frames, total frames) over the given spans.
    fn activity(&self, spans: impl IntoIterator<Item = (f64, f64)>) -> (usize, usize) {
        let mut active = 0;
        let mut total = 0;
        for (start, end) in spans {
            for i in self.frames(start, end) {
                total += 1;
                active += usize::from(self.system_active(i));
            }
        }
        (active, total)
    }

    /// Fraction of the spans' frames where the system channel is active; `None` when no frame overlaps.
    pub fn system_fraction(&self, spans: impl IntoIterator<Item = (f64, f64)>) -> Option<f64> {
        let (active, total) = self.activity(spans);
        (total > 0).then(|| active as f64 / total as f64)
    }
}

/// RMS level of samples in dBFS (full scale = 1.0), floored at [`SILENCE_DBFS`].
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return SILENCE_DBFS;
    }
    let mean_square = samples
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum::<f64>()
        / samples.len() as f64;
    if mean_square <= 0.0 {
        return SILENCE_DBFS;
    }
    clamp_db((10.0 * mean_square.log10()) as f32)
}

fn clamp_db(db: f32) -> f32 {
    if db.is_nan() {
        SILENCE_DBFS
    } else {
        db.clamp(SILENCE_DBFS, 0.0)
    }
}

/// Replace provider speaker labels with display names.
///
/// - With an energy track that has system audio: per provider speaker, the fraction of its speech
///   time where the system channel is active. The lowest fraction, if < 0.35, is `me_label`. The
///   rest are `them_label` (exactly one) or `them_label 1`, `them_label 2` … by first appearance.
///   Turns without a provider speaker (no diarization) are named one by one with the same test.
/// - Without energy, or with no system audio at all: `Speaker 1`, `Speaker 2` … by first appearance;
///   turns without a provider speaker stay unnamed.
pub fn name_speakers(
    turns: &[Turn],
    energy: Option<&EnergyTrack>,
    me_label: &str,
    them_label: &str,
) -> Vec<Turn> {
    let mut order: Vec<&str> = Vec::new();
    for turn in turns {
        if let Some(s) = turn.speaker.as_deref() {
            if !order.contains(&s) {
                order.push(s);
            }
        }
    }

    let energy = energy.filter(|e| e.has_system_audio());
    let names: Vec<String> = match energy {
        None => (1..=order.len()).map(|i| format!("Speaker {i}")).collect(),
        Some(track) => {
            let fractions: Vec<Option<f64>> = order
                .iter()
                .map(|s| {
                    track.system_fraction(
                        turns
                            .iter()
                            .filter(|t| t.speaker.as_deref() == Some(*s))
                            .map(|t| (t.start, t.end)),
                    )
                })
                .collect();
            let me = fractions
                .iter()
                .enumerate()
                .filter_map(|(i, f)| f.map(|f| (i, f)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|&(_, f)| f < ME_MAX_SYSTEM_FRACTION)
                .map(|(i, _)| i);
            let remote_count = order.len() - usize::from(me.is_some());
            let mut next_remote = 0;
            (0..order.len())
                .map(|i| {
                    if Some(i) == me {
                        me_label.to_string()
                    } else if remote_count == 1 {
                        them_label.to_string()
                    } else {
                        next_remote += 1;
                        format!("{them_label} {next_remote}")
                    }
                })
                .collect()
        }
    };

    turns
        .iter()
        .map(|turn| {
            let speaker = match turn.speaker.as_deref() {
                Some(s) => order.iter().position(|o| *o == s).map(|i| names[i].clone()),
                None => energy.and_then(|track| {
                    track.system_fraction([(turn.start, turn.end)]).map(|f| {
                        if f < ME_MAX_SYSTEM_FRACTION {
                            me_label.to_string()
                        } else {
                            them_label.to_string()
                        }
                    })
                }),
            };
            Turn {
                speaker,
                ..turn.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOUD: f32 = -20.0;
    const QUIET: f32 = -70.0;
    /// Mic bleed of the far end through the headset: present, but well below the system channel.
    const BLEED: f32 = -45.0;

    /// Build a 100 ms track from (seconds, mic_db, sys_db) spans; silence elsewhere.
    fn track(total_s: f64, spans: &[(f64, f64, f32, f32)]) -> EnergyTrack {
        let mut t = EnergyTrack::new(100);
        let frames = (total_s * 10.0) as usize;
        for i in 0..frames {
            let at = i as f64 / 10.0 + 0.05;
            let (mic, sys) = spans
                .iter()
                .find(|(s, e, _, _)| at >= *s && at < *e)
                .map(|&(_, _, m, s)| (m, s))
                .unwrap_or((QUIET, QUIET));
            t.push_db(mic, sys);
        }
        t
    }

    fn turn(speaker: Option<&str>, start: f64, end: f64) -> Turn {
        Turn {
            speaker: speaker.map(Into::into),
            start,
            end,
            text: format!("{start}-{end}"),
        }
    }

    fn names(turns: &[Turn]) -> Vec<Option<&str>> {
        turns.iter().map(|t| t.speaker.as_deref()).collect()
    }

    #[test]
    fn levels() {
        assert_eq!(rms_dbfs(&[]), SILENCE_DBFS);
        assert_eq!(rms_dbfs(&[0.0; 160]), SILENCE_DBFS);
        assert!((rms_dbfs(&[1.0, -1.0]) - 0.0).abs() < 1e-6);
        assert!((rms_dbfs(&[0.1; 160]) + 20.0).abs() < 1e-4);
        let mut t = EnergyTrack::default();
        t.push_db(rms_dbfs(&[0.5; 10]), rms_dbfs(&[]));
        t.push_db(f32::NEG_INFINITY, f32::NAN);
        assert_eq!(t.sys_db, [SILENCE_DBFS, SILENCE_DBFS]);
        assert_eq!(t.mic_db[1], SILENCE_DBFS);
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.starts_with(r#"{"frame_ms":100,"mic_db":["#), "{json}");
        assert_eq!(serde_json::from_str::<EnergyTrack>(&json).unwrap(), t);
    }

    #[test]
    fn activity_rule() {
        let mut t = EnergyTrack::new(100);
        t.push_db(LOUD, LOUD); // both loud: active (sys ≥ mic − 6)
        t.push_db(-10.0, -20.0); // mic much louder: inactive
        t.push_db(QUIET, -55.0); // below floor: inactive
        t.push_db(QUIET, -48.0); // above floor, mic silent: active
        assert_eq!(
            (0..4).map(|i| t.system_active(i)).collect::<Vec<_>>(),
            [true, false, false, true]
        );
        assert!(!t.system_active(9));
        assert_eq!(t.system_fraction([(0.0, 0.4)]), Some(0.5));
        assert_eq!(t.system_fraction([(5.0, 6.0)]), None);
        // A zero-length span still samples its frame.
        assert_eq!(t.system_fraction([(0.32, 0.32)]), Some(1.0));
    }

    #[test]
    fn me_only() {
        let energy = track(20.0, &[(0.0, 10.0, LOUD, QUIET), (12.0, 18.0, LOUD, QUIET)]);
        // Some system sound exists elsewhere (a notification), so the track counts as having system audio.
        let mut energy = energy;
        energy.push_db(QUIET, LOUD);
        let turns = vec![
            turn(Some("speaker_0"), 0.0, 10.0),
            turn(Some("speaker_0"), 12.0, 18.0),
        ];
        assert_eq!(
            names(&name_speakers(&turns, Some(&energy), "Me", "Them")),
            [Some("Me"), Some("Me")]
        );
    }

    #[test]
    fn me_and_one_remote() {
        let energy = track(
            30.0,
            &[
                (0.0, 8.0, LOUD, QUIET),
                (8.0, 20.0, BLEED, LOUD),
                (20.0, 30.0, LOUD, QUIET),
            ],
        );
        let turns = vec![
            turn(Some("speaker_1"), 0.2, 7.8),
            turn(Some("speaker_0"), 8.1, 19.9),
            turn(Some("speaker_1"), 20.1, 29.0),
        ];
        let named = name_speakers(&turns, Some(&energy), "Me", "Them");
        assert_eq!(names(&named), [Some("Me"), Some("Them"), Some("Me")]);
        assert_eq!(named[1].text, turns[1].text);
        assert_eq!((named[1].start, named[1].end), (8.1, 19.9));
    }

    #[test]
    fn me_and_two_remotes_with_imperfect_boundaries() {
        // Provider turn edges overlap the other channel by ~0.5 s; fractions stay decisive.
        let energy = track(
            40.0,
            &[
                (0.0, 10.0, BLEED, LOUD),
                (10.0, 20.0, LOUD, QUIET),
                (20.0, 30.0, BLEED, LOUD),
                (30.0, 40.0, BLEED, LOUD),
            ],
        );
        let turns = vec![
            turn(Some("speaker_2"), 0.0, 10.5),
            turn(Some("speaker_0"), 9.5, 20.5),
            turn(Some("speaker_1"), 19.5, 30.0),
            turn(Some("speaker_2"), 30.0, 40.0),
        ];
        let named = name_speakers(&turns, Some(&energy), "Me", "Them");
        assert_eq!(
            names(&named),
            [Some("Them 1"), Some("Me"), Some("Them 2"), Some("Them 1")]
        );
    }

    #[test]
    fn remote_only_has_no_me() {
        let energy = track(10.0, &[(0.0, 10.0, BLEED, LOUD)]);
        let turns = vec![
            turn(Some("speaker_0"), 0.0, 5.0),
            turn(Some("speaker_1"), 5.0, 10.0),
        ];
        assert_eq!(
            names(&name_speakers(&turns, Some(&energy), "Me", "Them")),
            [Some("Them 1"), Some("Them 2")]
        );
    }

    #[test]
    fn no_system_audio_uses_generic_names() {
        let energy = track(10.0, &[(0.0, 10.0, LOUD, QUIET)]);
        let turns = vec![
            turn(Some("b"), 0.0, 3.0),
            turn(Some("a"), 3.0, 6.0),
            turn(Some("b"), 6.0, 9.0),
            turn(None, 9.0, 10.0),
        ];
        let expected = [
            Some("Speaker 1"),
            Some("Speaker 2"),
            Some("Speaker 1"),
            None,
        ];
        assert_eq!(
            names(&name_speakers(&turns, Some(&energy), "Me", "Them")),
            expected
        );
        assert_eq!(names(&name_speakers(&turns, None, "Me", "Them")), expected);
        assert_eq!(
            names(&name_speakers(
                &turns,
                Some(&EnergyTrack::default()),
                "Me",
                "Them"
            )),
            expected
        );
    }

    #[test]
    fn undiarized_turns_are_named_one_by_one() {
        let energy = track(20.0, &[(0.0, 10.0, LOUD, QUIET), (10.0, 20.0, BLEED, LOUD)]);
        let turns = vec![
            turn(None, 0.0, 9.0),
            turn(None, 10.5, 19.0),
            turn(None, 25.0, 26.0),
        ];
        let named = name_speakers(&turns, Some(&energy), "Me", "Them");
        // The last turn lies beyond the track: unknown.
        assert_eq!(names(&named), [Some("Me"), Some("Them"), None]);
    }

    #[test]
    fn custom_labels_and_speaker_beyond_track() {
        let energy = track(10.0, &[(0.0, 5.0, LOUD, QUIET), (5.0, 10.0, BLEED, LOUD)]);
        let turns = vec![
            turn(Some("x"), 0.0, 5.0),
            turn(Some("y"), 5.0, 10.0),
            turn(Some("z"), 50.0, 60.0), // no energy coverage: never "me"
        ];
        let named = name_speakers(&turns, Some(&energy), "Simon", "Caller");
        assert_eq!(
            names(&named),
            [Some("Simon"), Some("Caller 1"), Some("Caller 2")]
        );
    }
}
