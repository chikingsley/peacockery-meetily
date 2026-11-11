//! Artifact → speaker turns (ARCHITECTURE.md §6.2).

use serde::{Deserialize, Serialize};

use crate::artifact::{Artifact, TimedText};

/// Consecutive units of one speaker closer than this (seconds) join one turn.
pub const MAX_GAP_SECONDS: f64 = 1.5;
/// Once a turn is this long (seconds) it is split at the next sentence end.
pub const SPLIT_AFTER_SECONDS: f64 = 30.0;
/// A turn is split here even without a sentence end (unpunctuated run-on output).
pub const HARD_MAX_SECONDS: f64 = 60.0;

/// One transcript turn. `speaker` is the provider label (`speaker_0`) before naming,
/// the display name (`Me`, `Them 1`, `Speaker 2`) after [`crate::speakers::name_speakers`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub speaker: Option<String>,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Which artifact track the turns were built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnSource {
    SpeakerTurns,
    Words,
    Segments,
    Text,
    Empty,
}

/// Turns from an artifact: `speaker_turns` if non-empty, else `words`, else `segments`, else the whole text.
///
/// Every source goes through the same grouping (same speaker, gap < 1.5 s, split at a sentence end
/// after 30 s, hard split at 60 s), so segment-sized speaker turns (Voxtral batch derives one per
/// segment) merge into readable turns too.
pub fn turns_from_artifact(artifact: &Artifact) -> Vec<Turn> {
    turns_with_source(artifact).0
}

/// [`turns_from_artifact`] plus the track it used.
pub fn turns_with_source(artifact: &Artifact) -> (Vec<Turn>, TurnSource) {
    let c = &artifact.content;
    let non_empty = |items: &[TimedText]| items.iter().any(|i| !i.text.trim().is_empty());
    if non_empty(&c.speaker_turns.items) {
        return (
            group_units(&c.speaker_turns.items),
            TurnSource::SpeakerTurns,
        );
    }
    if non_empty(&c.words.items) {
        return (group_units(&c.words.items), TurnSource::Words);
    }
    if non_empty(&c.segments.items) {
        return (group_units(&c.segments.items), TurnSource::Segments);
    }
    let text = normalize_spaces(&artifact.text);
    if text.is_empty() {
        return (Vec::new(), TurnSource::Empty);
    }
    let end = artifact.duration_seconds().unwrap_or(0.0).max(0.0);
    (
        vec![Turn {
            speaker: None,
            start: 0.0,
            end,
            text,
        }],
        TurnSource::Text,
    )
}

/// Group timed units into turns. Null times inherit the previous end.
pub fn group_units(units: &[TimedText]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    let mut clock = 0.0_f64;
    for unit in units {
        let start = unit
            .start_seconds
            .filter(|s| s.is_finite())
            .unwrap_or(clock)
            .max(0.0);
        let end = unit
            .end_seconds
            .filter(|e| e.is_finite())
            .unwrap_or(start)
            .max(start);
        clock = clock.max(end);
        if unit.text.trim().is_empty() {
            continue;
        }
        let joins = turns.last().is_some_and(|cur| {
            let length = cur.end - cur.start;
            cur.speaker == unit.speaker
                && start - cur.end < MAX_GAP_SECONDS
                && !(length >= SPLIT_AFTER_SECONDS && ends_sentence(&cur.text))
                && length < HARD_MAX_SECONDS
        });
        match turns.last_mut() {
            Some(cur) if joins => {
                push_token(&mut cur.text, &unit.text);
                cur.end = cur.end.max(end);
            }
            _ => {
                let mut text = String::new();
                push_token(&mut text, &unit.text);
                turns.push(Turn {
                    speaker: unit.speaker.clone(),
                    start,
                    end,
                    text,
                });
            }
        }
    }
    turns
}

/// True when the text ends a sentence (`.`, `?`, `!`, `…`, optionally followed by a closing quote or bracket).
pub fn ends_sentence(text: &str) -> bool {
    let trimmed = text
        .trim_end()
        .trim_end_matches(['"', '\'', ')', ']', '”', '’']);
    trimmed.ends_with(['.', '?', '!', '…'])
}

/// Append a word/segment token with natural spacing: tokens that carry their own leading space
/// get one space; punctuation and contractions attach; everything else is space-separated.
pub fn push_token(buf: &mut String, token: &str) {
    let leading_space = token.starts_with(char::is_whitespace);
    let t = token.trim();
    if t.is_empty() {
        return;
    }
    if buf.is_empty() {
        buf.push_str(t);
        return;
    }
    let attaches = !leading_space
        && (starts_with_closing_punct(t)
            || is_contraction(t)
            || buf.ends_with(['(', '[', '{', '“']));
    if !attaches && !buf.ends_with(char::is_whitespace) {
        buf.push(' ');
    }
    buf.push_str(t);
}

fn starts_with_closing_punct(t: &str) -> bool {
    t.starts_with([',', '.', '!', '?', ';', ':', '%', ')', ']', '}', '…', '”'])
}

fn is_contraction(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    [
        "'s", "'t", "'re", "'ve", "'ll", "'d", "'m", "’s", "’t", "’re", "’ve", "’ll", "’d", "’m",
        "n't", "n’t",
    ]
    .iter()
    .any(|&c| {
        lower == c || (lower.starts_with(c) && !lower[c.len()..].starts_with(char::is_alphanumeric))
    })
}

/// Collapse runs of whitespace to single spaces and trim.
pub fn normalize_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn unit(speaker: Option<&str>, start: f64, end: f64, text: &str) -> TimedText {
        TimedText {
            start_seconds: Some(start),
            end_seconds: Some(end),
            text: text.into(),
            speaker: speaker.map(Into::into),
        }
    }

    fn job_result(name: &str) -> Artifact {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let job: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        Artifact::from_value(job["result"].clone()).unwrap()
    }

    fn squash(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn assert_well_formed(turns: &[Turn]) {
        for pair in turns.windows(2) {
            assert!(pair[0].start <= pair[1].start, "{pair:?}");
        }
        for t in turns {
            assert!(
                t.end >= t.start && !t.text.is_empty() && t.text == t.text.trim(),
                "{t:?}"
            );
            assert!(!t.text.contains("  "), "{t:?}");
        }
    }

    #[test]
    fn token_joining() {
        let mut s = String::new();
        for t in [
            "Hi", ",", "it", "'s", "Dana", ".", " How", "are", "you", "?", "(", "laughs", ")",
            "don", "n't",
        ] {
            push_token(&mut s, t);
        }
        assert_eq!(s, "Hi, it's Dana. How are you? (laughs) donn't");
        let mut s = String::from("Hello.");
        push_token(&mut s, " World");
        push_token(&mut s, "  ");
        push_token(&mut s, "'tis");
        assert_eq!(s, "Hello. World 'tis");
    }

    #[test]
    fn sentence_ends() {
        assert!(ends_sentence("Okay."));
        assert!(ends_sentence("Really?\" "));
        assert!(ends_sentence("Wait…"));
        assert!(!ends_sentence("and then,"));
        assert!(!ends_sentence(""));
    }

    #[test]
    fn groups_by_speaker_and_gap() {
        let units = vec![
            unit(Some("speaker_0"), 0.0, 0.4, "Hi,"),
            unit(Some("speaker_0"), 0.5, 0.9, "there."),
            unit(Some("speaker_1"), 1.0, 1.4, "Hello"),
            unit(Some("speaker_1"), 1.5, 1.9, "!"),
            unit(Some("speaker_1"), 3.5, 3.9, "Later"), // gap 1.6 s → new turn
            unit(Some("speaker_1"), 4.0, 4.2, "   "),   // blank: skipped
        ];
        let turns = group_units(&units);
        let texts: Vec<_> = turns
            .iter()
            .map(|t| (t.speaker.as_deref(), t.text.as_str(), t.start, t.end))
            .collect();
        assert_eq!(
            texts,
            [
                (Some("speaker_0"), "Hi, there.", 0.0, 0.9),
                (Some("speaker_1"), "Hello!", 1.0, 1.9),
                (Some("speaker_1"), "Later", 3.5, 3.9)
            ]
        );
    }

    #[test]
    fn splits_long_turns_at_sentence_end() {
        // One speaker talking continuously: 0.5 s words, a sentence end every 8 words.
        let mut units = Vec::new();
        for i in 0..200 {
            let start = i as f64 * 0.5;
            let text = if i % 8 == 7 { "end." } else { "word" };
            units.push(unit(Some("speaker_0"), start, start + 0.4, text));
        }
        let turns = group_units(&units);
        assert!(turns.len() >= 3, "{}", turns.len());
        for t in &turns[..turns.len() - 1] {
            assert!(ends_sentence(&t.text), "{t:?}");
            let len = t.end - t.start;
            assert!(
                (SPLIT_AFTER_SECONDS..SPLIT_AFTER_SECONDS + 5.0).contains(&len),
                "{len}"
            );
        }
        assert_eq!(
            turns
                .iter()
                .map(|t| t.text.split(' ').count())
                .sum::<usize>(),
            200
        );
    }

    #[test]
    fn hard_split_without_punctuation() {
        let units: Vec<_> = (0..300)
            .map(|i| unit(None, i as f64 * 0.5, i as f64 * 0.5 + 0.4, "word"))
            .collect();
        let turns = group_units(&units);
        assert!(
            turns
                .iter()
                .all(|t| t.end - t.start <= HARD_MAX_SECONDS + 0.5),
            "{turns:?}"
        );
        assert!(turns.len() >= 3);
    }

    #[test]
    fn null_times_inherit_previous_end() {
        let units = vec![
            unit(Some("a"), 1.0, 2.0, "one."),
            TimedText {
                start_seconds: None,
                end_seconds: None,
                text: " two".into(),
                speaker: Some("a".into()),
            },
            TimedText {
                start_seconds: None,
                end_seconds: None,
                text: " three".into(),
                speaker: Some("b".into()),
            },
        ];
        let turns = group_units(&units);
        assert_eq!(turns.len(), 2);
        assert_eq!((turns[0].text.as_str(), turns[0].end), ("one. two", 2.0));
        assert_eq!((turns[1].start, turns[1].end), (2.0, 2.0));
    }

    #[test]
    fn scribe_batch_groups_words() {
        let a = job_result("b-scribe.json");
        let (turns, source) = turns_with_source(&a);
        assert_eq!(source, TurnSource::Words);
        assert_well_formed(&turns);
        assert_eq!(
            squash(&turns.iter().map(|t| t.text.as_str()).collect::<String>()),
            squash(&a.text)
        );
        assert!(turns.len() > 1 && turns.len() < 40, "{}", turns.len());
        assert!(turns
            .iter()
            .all(|t| t.speaker.as_deref() == Some("speaker_0")));
        assert!(turns.iter().all(|t| t.end - t.start < HARD_MAX_SECONDS));
        assert!(turns[0].text.starts_with("Hi, thanks for calling"));
    }

    #[test]
    fn voxtral_batch_uses_speaker_turns() {
        let a = job_result("b-voxsmall.json");
        let (turns, source) = turns_with_source(&a);
        assert_eq!(source, TurnSource::SpeakerTurns);
        assert_well_formed(&turns);
        assert!(turns.len() < 27, "adjacent same-speaker segments merge");
        let expected: String = a
            .content
            .speaker_turns
            .items
            .iter()
            .map(|i| i.text.as_str())
            .collect();
        assert_eq!(
            squash(&turns.iter().map(|t| t.text.as_str()).collect::<String>()),
            squash(&expected)
        );
        let speakers: std::collections::BTreeSet<_> =
            turns.iter().filter_map(|t| t.speaker.clone()).collect();
        // The synthetic dialogue came back as one provider speaker from Voxtral Small too.
        assert_eq!(speakers.into_iter().collect::<Vec<_>>(), ["speaker_1"]);
        for pair in turns.windows(2) {
            let gap = pair[1].start - pair[0].end;
            assert!(
                pair[0].speaker != pair[1].speaker
                    || gap >= MAX_GAP_SECONDS
                    || pair[0].end - pair[0].start >= SPLIT_AFTER_SECONDS
            );
        }
    }

    #[test]
    fn falls_back_to_segments_then_text() {
        let a = job_result("b-voxsmall.json");
        let mut raw = a.raw.clone();
        raw["content"]["speaker_turns"]["items"] = Value::Array(vec![]);
        let segments_only = Artifact::from_value(raw.clone()).unwrap();
        assert_eq!(turns_with_source(&segments_only).1, TurnSource::Segments);

        raw["content"]["segments"]["items"] = Value::Array(vec![]);
        let text_only = Artifact::from_value(raw).unwrap();
        let (turns, source) = turns_with_source(&text_only);
        assert_eq!(source, TurnSource::Text);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].end, 77.0);
        assert_eq!(turns[0].text, normalize_spaces(&a.text));

        let empty: Artifact = serde_json::from_str(r#"{"text":"  "}"#).unwrap();
        assert_eq!(turns_with_source(&empty), (vec![], TurnSource::Empty));
    }
}
