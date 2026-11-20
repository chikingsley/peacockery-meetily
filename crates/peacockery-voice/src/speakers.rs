//! Speaker naming for the final transcript (ARCHITECTURE.md §6.3) and channel level measurement.

use crate::turns::Turn;

/// Level reported for digital silence.
pub const SILENCE_DBFS: f32 = -100.0;

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
    let db = (10.0 * mean_square.log10()) as f32;
    if db.is_nan() {
        SILENCE_DBFS
    } else {
        db.clamp(SILENCE_DBFS, 0.0)
    }
}

/// Replace provider speaker labels with `Speaker 1`, `Speaker 2` … by first appearance.
/// Turns without a provider speaker stay unnamed.
pub fn name_speakers(turns: &[Turn]) -> Vec<Turn> {
    let mut order: Vec<&str> = Vec::new();
    turns
        .iter()
        .map(|turn| {
            let speaker = turn.speaker.as_deref().map(|s| {
                let index = order.iter().position(|o| *o == s).unwrap_or_else(|| {
                    order.push(s);
                    order.len() - 1
                });
                format!("Speaker {}", index + 1)
            });
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

    fn turn(speaker: Option<&str>, start: f64, end: f64) -> Turn {
        Turn {
            speaker: speaker.map(Into::into),
            start,
            end,
            text: format!("{start}-{end}"),
        }
    }

    #[test]
    fn levels() {
        assert_eq!(rms_dbfs(&[]), SILENCE_DBFS);
        assert_eq!(rms_dbfs(&[0.0; 160]), SILENCE_DBFS);
        assert!((rms_dbfs(&[1.0, -1.0]) - 0.0).abs() < 1e-6);
        assert!((rms_dbfs(&[0.1; 160]) + 20.0).abs() < 1e-4);
        assert_eq!(rms_dbfs(&[f32::NAN; 4]), SILENCE_DBFS);
    }

    #[test]
    fn speakers_are_numbered_by_first_appearance() {
        let turns = vec![
            turn(Some("b"), 0.0, 3.0),
            turn(Some("a"), 3.0, 6.0),
            turn(Some("b"), 6.0, 9.0),
            turn(None, 9.0, 10.0),
        ];
        let named = name_speakers(&turns);
        assert_eq!(
            named
                .iter()
                .map(|t| t.speaker.as_deref())
                .collect::<Vec<_>>(),
            [
                Some("Speaker 1"),
                Some("Speaker 2"),
                Some("Speaker 1"),
                None
            ]
        );
        assert_eq!(named[1].text, turns[1].text);
        assert_eq!((named[1].start, named[1].end), (3.0, 6.0));
    }
}
