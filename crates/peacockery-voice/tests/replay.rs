//! Replays every recorded realtime fixture through `LineBuilder`, using `_t` (seconds since connect) as the clock.

use peacockery_voice::live::MAX_LINE_SECONDS;
use peacockery_voice::{
    Channel, LineBuilder, LineBuilderConfig, LineStyle, LineUpdate, LiveMode, RealtimeEvent,
    SpeakerLabels,
};
use serde_json::Value;

struct Replay {
    finals: Vec<(Option<String>, String, f64, f64)>,
    updates: Vec<LineUpdate>,
    provider_text: String,
}

fn replay(name: &str, mode: LiveMode, channel: Channel, diarize: bool) -> Replay {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let events: Vec<Value> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let model = events[0]["model"].as_str().unwrap().to_string();
    let speakers = SpeakerLabels::for_session(mode, channel, diarize, "Me", "Them");
    let mut builder = LineBuilder::new(LineBuilderConfig::new(channel, &model, speakers));
    let mut updates = Vec::new();
    let mut clock = 0.0_f64;
    let mut provider_text = String::new();
    for raw in events {
        let t = raw["_t"].as_f64().unwrap();
        // The app ticks the builder ~10 times a second between events.
        while clock + 0.1 < t {
            clock += 0.1;
            updates.extend(builder.tick(clock));
        }
        clock = t;
        let event = RealtimeEvent::from_value(raw).unwrap();
        if let RealtimeEvent::SessionCompleted { result, .. } = &event {
            provider_text.clone_from(&result.text);
        }
        updates.extend(builder.on_event(&event, t));
    }
    let finals = updates
        .iter()
        .filter_map(|u| match u {
            LineUpdate::Final {
                speaker,
                text,
                start,
                end,
                channel: c,
            } => {
                assert_eq!(*c, channel);
                Some((speaker.clone(), text.clone(), *start, *end))
            }
            LineUpdate::Preview { channel: c, .. } => {
                assert_eq!(*c, channel);
                None
            }
        })
        .collect();
    Replay {
        finals,
        updates,
        provider_text,
    }
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn assert_sensible(name: &str, r: &Replay, min_lines: usize, max_lines: usize) {
    assert!(!r.provider_text.trim().is_empty(), "{name}: provider text");
    assert!(
        (min_lines..=max_lines).contains(&r.finals.len()),
        "{name}: {} lines\n{:#?}",
        r.finals.len(),
        r.finals
    );
    let mut prev_end = 0.0;
    for (speaker, text, start, end) in &r.finals {
        assert!(
            !text.is_empty() && text == text.trim() && !text.contains("  "),
            "{name}: {text:?}"
        );
        assert!(
            *start >= prev_end - 1e-9 && end >= start,
            "{name}: times {start}..{end} after {prev_end}"
        );
        prev_end = *end;
        let _ = speaker;
    }
    for pair in r.finals.windows(2) {
        assert_ne!(pair[0].1, pair[1].1, "{name}: duplicate consecutive line");
    }
    // Every word exactly once: the lines cover the provider's final text, modulo whitespace.
    let joined: String = r
        .finals
        .iter()
        .map(|f| f.1.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(squash(&joined), squash(&r.provider_text), "{name}");
    // The preview is cleared at the end (the last preview update is an empty one).
    let last_preview = r.updates.iter().rev().find_map(|u| match u {
        LineUpdate::Preview { text, .. } => Some(text.as_str()),
        LineUpdate::Final { .. } => None,
    });
    assert_eq!(last_preview, Some(""), "{name}");
    // Recording-clock times stay within the 77 s call (+ connect latency and the trailing silence).
    assert!(r.finals.last().unwrap().3 < 85.0, "{name}");
}

fn assert_fragment_lines(name: &str, r: &Replay) {
    for (_, text, start, end) in &r.finals {
        assert!(end - start <= MAX_LINE_SECONDS + 1e-9, "{name}: {text}");
    }
}

#[test]
fn voxtral_plain_split_mic() {
    let r = replay("vox-plain.json", LiveMode::Split, Channel::Mic, false);
    assert_sensible("vox-plain", &r, 8, 40);
    assert_fragment_lines("vox-plain", &r);
    assert!(r.finals.iter().all(|f| f.0.as_deref() == Some("Me")));
    // Most lines end at a sentence boundary rather than mid-sentence.
    let sentence_ends = r
        .finals
        .iter()
        .filter(|f| f.1.ends_with(['.', '?', '!']))
        .count();
    assert!(sentence_ends * 10 >= r.finals.len() * 7, "{:#?}", r.finals);
}

#[test]
fn voxtral_turn_events_mixed() {
    let r = replay("vox-turn.json", LiveMode::Mixed, Channel::Mixed, false);
    assert_sensible("vox-turn", &r, 8, 40);
    assert_fragment_lines("vox-turn", &r);
    assert!(r.finals.iter().all(|f| f.0.is_none()));
}

#[test]
fn voxtral_diarized_mixed() {
    let r = replay("vox-diar.json", LiveMode::Mixed, Channel::Mixed, true);
    assert_sensible("vox-diar", &r, 8, 60);
    assert_fragment_lines("vox-diar", &r);
    assert!(r
        .finals
        .iter()
        .all(|f| f.0.as_deref().is_some_and(|s| s.starts_with("Speaker "))));
    let distinct: std::collections::BTreeSet<_> =
        r.finals.iter().filter_map(|f| f.0.clone()).collect();
    assert!(distinct.contains("Speaker 1"));
}

#[test]
fn voxtral_diarized_split_system() {
    let r = replay("vox-diar.json", LiveMode::Split, Channel::System, true);
    assert_sensible("vox-diar/system", &r, 8, 60);
    assert!(r
        .finals
        .iter()
        .all(|f| f.0.as_deref().is_some_and(|s| s.starts_with("Them "))));
}

#[test]
fn nova_interim_split_system() {
    let r = replay("nova.json", LiveMode::Split, Channel::System, false);
    assert_sensible("nova", &r, 20, 40);
    assert!(r.finals.iter().all(|f| f.0.as_deref() == Some("Them")));
    // Nova committed lines carry provider segment times.
    assert_eq!((r.finals[0].2, r.finals[0].3), (0.0, 0.53));
    let previews = r
        .updates
        .iter()
        .filter(|u| matches!(u, LineUpdate::Preview { text, .. } if !text.is_empty()))
        .count();
    assert!(previews >= 40, "{previews}");
}

#[test]
fn scribe_interim_mixed() {
    let r = replay("scribe-rt.json", LiveMode::Mixed, Channel::Mixed, false);
    assert_sensible("scribe-rt", &r, 3, 3);
    assert!((r.finals[0].2 - 0.06).abs() < 1e-9);
    assert_eq!(
        LineStyle::for_model("elevenlabs-scribe_v2"),
        LineStyle::Interim
    );
}
