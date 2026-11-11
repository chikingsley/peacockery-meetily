//! Live transcript lines from realtime events (ARCHITECTURE.md §5).
//!
//! [`LineBuilder`] is pure: it never reads a clock. Every call passes `sent`, the recording-clock
//! time (seconds) of the audio sent to this session so far; a fragment's audio time is taken as
//! `sent − streaming_delay`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::catalog::ModelFamily;
use crate::realtime::{RealtimeEvent, TranscriptEvent};
use crate::turns::{ends_sentence, normalize_spaces};

/// Fragment lines close after this much silence from the provider (seconds of sent audio).
pub const IDLE_SECONDS: f64 = 1.5;
/// A fragment line that ends a sentence closes once it is this long (seconds) …
pub const SENTENCE_MIN_SECONDS: f64 = 6.0;
/// … or has this many words.
pub const SENTENCE_MIN_WORDS: usize = 18;
/// Fragment lines never grow past this (seconds).
pub const MAX_LINE_SECONDS: f64 = 30.0;
/// Default provider streaming delay (seconds).
pub const DEFAULT_STREAMING_DELAY: f64 = 0.5;

/// Which audio a session transcribes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Mic,
    System,
    Mixed,
}

/// `live_mode` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiveMode {
    /// mic → "Me" session, system → "Them" session.
    Split,
    /// one session on the mix.
    Mixed,
}

/// How the provider's events are shaped, decided by model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineStyle {
    /// Nova, Scribe, Cohere: `interim` = preview, `committed` = final line.
    Interim,
    /// Voxtral: delta or diarized committed fragments appended into lines.
    Fragment,
}

impl LineStyle {
    pub fn for_model(model_id: &str) -> Self {
        match ModelFamily::of(model_id) {
            ModelFamily::VoxtralRealtime3 | ModelFamily::Mistral => LineStyle::Fragment,
            _ => LineStyle::Interim,
        }
    }
}

/// Speaker label for a session's lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeakerLabels {
    /// No label.
    None,
    /// Every line gets this label (split mode).
    Fixed(String),
    /// `<prefix> 1`, `<prefix> 2` … per provider speaker, numbered by first appearance;
    /// lines without a provider speaker get `fallback`.
    Numbered {
        prefix: String,
        fallback: Option<String>,
    },
}

impl SpeakerLabels {
    /// The §5 rule: split mic → `me`; split system → `them` (or `them 1/2/…` with diarize);
    /// mixed + diarize → `Speaker 1/2/…`; otherwise none.
    pub fn for_session(
        mode: LiveMode,
        channel: Channel,
        diarize: bool,
        me: &str,
        them: &str,
    ) -> Self {
        match (mode, channel, diarize) {
            (LiveMode::Split, Channel::Mic, _) => SpeakerLabels::Fixed(me.into()),
            (LiveMode::Split, _, false) => SpeakerLabels::Fixed(them.into()),
            (LiveMode::Split, _, true) => SpeakerLabels::Numbered {
                prefix: them.into(),
                fallback: Some(them.into()),
            },
            (LiveMode::Mixed, _, true) => SpeakerLabels::Numbered {
                prefix: "Speaker".into(),
                fallback: None,
            },
            (LiveMode::Mixed, _, false) => SpeakerLabels::None,
        }
    }
}

/// Builder configuration for one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineBuilderConfig {
    pub channel: Channel,
    pub style: LineStyle,
    pub speakers: SpeakerLabels,
    /// Seconds between audio being sent and its fragment arriving.
    pub streaming_delay: f64,
    /// Recording time at which this session's audio starts (non-zero after a reconnect);
    /// added to provider segment times.
    pub session_offset: f64,
}

impl LineBuilderConfig {
    pub fn new(channel: Channel, model_id: &str, speakers: SpeakerLabels) -> Self {
        Self {
            channel,
            style: LineStyle::for_model(model_id),
            speakers,
            streaming_delay: DEFAULT_STREAMING_DELAY,
            session_offset: 0.0,
        }
    }
}

/// Output of the builder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LineUpdate {
    /// Replace this channel's preview line; empty text clears it.
    Preview {
        channel: Channel,
        speaker: Option<String>,
        text: String,
    },
    /// A finished line (recording-clock seconds).
    Final {
        channel: Channel,
        speaker: Option<String>,
        text: String,
        start: f64,
        end: f64,
    },
}

#[derive(Debug, Clone)]
struct OpenLine {
    speaker: Option<String>,
    text: String,
    start: f64,
    end: f64,
    /// `sent` of the last fragment (idle detection).
    last_sent: f64,
}

/// Realtime events → preview + final lines for one session.
#[derive(Debug, Clone)]
pub struct LineBuilder {
    config: LineBuilderConfig,
    open: Option<OpenLine>,
    /// Interim style: start time of the utterance currently previewed.
    interim_start: Option<f64>,
    /// Interim style: latest hypothesis not yet committed.
    interim_text: String,
    preview: String,
    preview_speaker: Option<String>,
    last_end: f64,
    numbering: HashMap<String, usize>,
    finished: bool,
}

impl LineBuilder {
    pub fn new(config: LineBuilderConfig) -> Self {
        Self {
            config,
            open: None,
            interim_start: None,
            interim_text: String::new(),
            preview: String::new(),
            preview_speaker: None,
            last_end: 0.0,
            numbering: HashMap::new(),
            finished: false,
        }
    }

    pub fn config(&self) -> &LineBuilderConfig {
        &self.config
    }

    /// Feed one event. `sent` = recording time of the audio sent so far on this session.
    pub fn on_event(&mut self, event: &RealtimeEvent, sent: f64) -> Vec<LineUpdate> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        match (self.config.style, event) {
            (_, RealtimeEvent::SessionCompleted { .. }) => {
                // The provider's result holds only committed text: an uncommitted hypothesis
                // at a clean end was dropped by the provider, so drop it here too.
                self.interim_text.clear();
                return self.finish(sent);
            }
            (_, RealtimeEvent::SessionFailed { .. }) => return self.finish(sent),
            (LineStyle::Fragment, RealtimeEvent::Delta(e) | RealtimeEvent::Committed(e)) => {
                self.close_if_idle(sent, &mut out);
                self.fragments(e, sent, &mut out);
            }
            (LineStyle::Fragment, RealtimeEvent::Interim(e)) => {
                // Not sent by Voxtral; show it without disturbing the open line.
                self.set_preview(
                    self.open_preview_speaker(),
                    &format!("{}{}", self.open_text(), e.text),
                    &mut out,
                );
            }
            (LineStyle::Interim, RealtimeEvent::Interim(e)) => self.interim(e, sent, &mut out),
            (LineStyle::Interim, RealtimeEvent::Committed(e)) => self.committed(e, sent, &mut out),
            (LineStyle::Interim, RealtimeEvent::Delta(e)) => {
                // Unexpected for this style: treat the delta as an extension of the hypothesis.
                let text = format!("{}{}", self.interim_text, e.text);
                let ev = TranscriptEvent { text, ..e.clone() };
                self.interim(&ev, sent, &mut out);
            }
            _ => self.close_if_idle(sent, &mut out),
        }
        out
    }

    /// Time passes without events (call a few times per second): closes idle fragment lines.
    pub fn tick(&mut self, sent: f64) -> Vec<LineUpdate> {
        let mut out = Vec::new();
        if !self.finished {
            self.close_if_idle(sent, &mut out);
        }
        out
    }

    /// End of session: emit what is pending (the open fragment line, or an uncommitted interim
    /// hypothesis when the session did not complete cleanly) and clear the preview. Idempotent.
    pub fn finish(&mut self, sent: f64) -> Vec<LineUpdate> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        self.finished = true;
        self.close_open(&mut out);
        let pending = std::mem::take(&mut self.interim_text);
        if !pending.trim().is_empty() {
            let start = self
                .interim_start
                .take()
                .unwrap_or_else(|| self.audio_time(sent));
            let end = self.audio_time(sent);
            let speaker = self.label(None);
            self.emit_final(speaker, &pending, start, end, &mut out);
        }
        self.set_preview(None, "", &mut out);
        out
    }

    fn audio_time(&self, sent: f64) -> f64 {
        (sent - self.config.streaming_delay)
            .max(self.config.session_offset)
            .max(0.0)
    }

    fn label(&mut self, provider: Option<&str>) -> Option<String> {
        match &self.config.speakers {
            SpeakerLabels::None => None,
            SpeakerLabels::Fixed(name) => Some(name.clone()),
            SpeakerLabels::Numbered { prefix, fallback } => match provider {
                Some(id) => {
                    let next = self.numbering.len() + 1;
                    let n = *self.numbering.entry(id.to_string()).or_insert(next);
                    Some(format!("{prefix} {n}"))
                }
                None => fallback.clone(),
            },
        }
    }

    fn open_text(&self) -> &str {
        self.open.as_ref().map(|l| l.text.as_str()).unwrap_or("")
    }

    fn open_preview_speaker(&self) -> Option<String> {
        self.open
            .as_ref()
            .and_then(|l| l.speaker.clone())
            .or_else(|| match &self.config.speakers {
                SpeakerLabels::Fixed(name) => Some(name.clone()),
                _ => None,
            })
    }

    fn set_preview(&mut self, speaker: Option<String>, text: &str, out: &mut Vec<LineUpdate>) {
        let text = normalize_spaces(text);
        if text == self.preview && (text.is_empty() || speaker == self.preview_speaker) {
            return;
        }
        self.preview.clone_from(&text);
        self.preview_speaker.clone_from(&speaker);
        out.push(LineUpdate::Preview {
            channel: self.config.channel,
            speaker,
            text,
        });
    }

    fn emit_final(
        &mut self,
        speaker: Option<String>,
        text: &str,
        start: f64,
        end: f64,
        out: &mut Vec<LineUpdate>,
    ) {
        let text = normalize_spaces(text);
        if text.is_empty() {
            return;
        }
        // Keep lines ordered on the recording clock.
        let start = start.max(self.last_end).max(0.0);
        let end = end.max(start);
        self.last_end = end;
        out.push(LineUpdate::Final {
            channel: self.config.channel,
            speaker,
            text,
            start,
            end,
        });
    }

    // ---- fragment style -------------------------------------------------------------------

    fn fragments(&mut self, event: &TranscriptEvent, sent: f64, out: &mut Vec<LineUpdate>) {
        let t = self.audio_time(sent);
        // Diarized commits carry one segment per speaker run; plain deltas carry only text.
        let pieces: Vec<(Option<String>, String)> =
            if event.segments.iter().any(|s| !s.text.is_empty()) {
                event
                    .segments
                    .iter()
                    .map(|s| (s.speaker.clone(), s.text.clone()))
                    .collect()
            } else {
                vec![(None, event.text.clone())]
            };
        for (provider_speaker, text) in pieces {
            if text.is_empty() {
                continue;
            }
            let speaker = self.label(provider_speaker.as_deref());
            if self.open.as_ref().is_some_and(|l| l.speaker != speaker) {
                self.close_open(out);
            }
            let line = self.open.get_or_insert_with(|| OpenLine {
                speaker: speaker.clone(),
                text: String::new(),
                start: t,
                end: t,
                last_sent: sent,
            });
            if line.text.trim().is_empty() {
                // Line starts at the first fragment that carries words.
                line.start = t;
                line.text.clear();
            }
            line.text.push_str(&text);
            line.end = t;
            line.last_sent = sent;

            let duration = line.end - line.start;
            let words = line.text.split_whitespace().count();
            let sentence_done = ends_sentence(&line.text)
                && (duration >= SENTENCE_MIN_SECONDS || words >= SENTENCE_MIN_WORDS);
            if sentence_done || duration >= MAX_LINE_SECONDS {
                self.close_open(out);
            }
        }
        let preview_speaker = self.open_preview_speaker();
        let preview = self.open_text().to_string();
        self.set_preview(preview_speaker, &preview, out);
    }

    fn close_if_idle(&mut self, sent: f64, out: &mut Vec<LineUpdate>) {
        if self
            .open
            .as_ref()
            .is_some_and(|l| sent - l.last_sent >= IDLE_SECONDS)
        {
            self.close_open(out);
            self.set_preview(None, "", out);
        }
    }

    fn close_open(&mut self, out: &mut Vec<LineUpdate>) {
        if let Some(line) = self.open.take() {
            self.emit_final(line.speaker, &line.text, line.start, line.end, out);
        }
    }

    // ---- interim style --------------------------------------------------------------------

    fn segment_span(&self, event: &TranscriptEvent) -> Option<(f64, f64)> {
        let starts = event.segments.iter().filter_map(|s| s.start_seconds);
        let ends = event.segments.iter().filter_map(|s| s.end_seconds);
        let start = starts.fold(f64::INFINITY, f64::min);
        let end = ends.fold(f64::NEG_INFINITY, f64::max);
        (start.is_finite() && end.is_finite()).then_some((
            start + self.config.session_offset,
            end + self.config.session_offset,
        ))
    }

    fn interim(&mut self, event: &TranscriptEvent, sent: f64, out: &mut Vec<LineUpdate>) {
        if event.text.trim().is_empty() {
            return;
        }
        if self.interim_start.is_none() {
            let start = self
                .segment_span(event)
                .map(|(s, _)| s)
                .unwrap_or_else(|| self.audio_time(sent));
            self.interim_start = Some(start);
        }
        self.interim_text.clone_from(&event.text);
        let speaker = self.label(None);
        self.set_preview(speaker, &event.text, out);
    }

    fn committed(&mut self, event: &TranscriptEvent, sent: f64, out: &mut Vec<LineUpdate>) {
        let (start, end) = self.segment_span(event).unwrap_or_else(|| {
            let end = self.audio_time(sent);
            (self.interim_start.unwrap_or(end), end)
        });
        let provider = event.segments.iter().find_map(|s| s.speaker.clone());
        let speaker = self.label(provider.as_deref());
        self.emit_final(speaker, &event.text, start, end, out);
        self.interim_start = None;
        self.interim_text.clear();
        self.set_preview(None, "", out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::LiveSegment;

    fn delta(text: &str) -> RealtimeEvent {
        RealtimeEvent::Delta(TranscriptEvent {
            text: text.into(),
            ..Default::default()
        })
    }

    fn diar(speaker: &str, text: &str) -> RealtimeEvent {
        RealtimeEvent::Committed(TranscriptEvent {
            text: text.into(),
            segments: vec![LiveSegment {
                text: text.into(),
                speaker: Some(speaker.into()),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn finals(updates: &[LineUpdate]) -> Vec<(Option<&str>, &str, f64, f64)> {
        updates
            .iter()
            .filter_map(|u| match u {
                LineUpdate::Final {
                    speaker,
                    text,
                    start,
                    end,
                    ..
                } => Some((speaker.as_deref(), text.as_str(), *start, *end)),
                _ => None,
            })
            .collect()
    }

    fn fragment_builder(speakers: SpeakerLabels) -> LineBuilder {
        LineBuilder::new(LineBuilderConfig::new(
            Channel::Mic,
            "mistral-voxtral-mini-transcribe-realtime-3",
            speakers,
        ))
    }

    #[test]
    fn style_and_labels() {
        assert_eq!(
            LineStyle::for_model("mistral-voxtral-mini-transcribe-realtime-3"),
            LineStyle::Fragment
        );
        assert_eq!(LineStyle::for_model("deepgram-nova-3"), LineStyle::Interim);
        assert_eq!(
            LineStyle::for_model("elevenlabs-scribe_v2"),
            LineStyle::Interim
        );
        assert_eq!(
            LineStyle::for_model("cohere-transcribe"),
            LineStyle::Interim
        );
        assert_eq!(
            SpeakerLabels::for_session(LiveMode::Split, Channel::Mic, true, "Me", "Them"),
            SpeakerLabels::Fixed("Me".into())
        );
        assert_eq!(
            SpeakerLabels::for_session(LiveMode::Split, Channel::System, false, "Me", "Them"),
            SpeakerLabels::Fixed("Them".into())
        );
        assert_eq!(
            SpeakerLabels::for_session(LiveMode::Split, Channel::System, true, "Me", "Them"),
            SpeakerLabels::Numbered {
                prefix: "Them".into(),
                fallback: Some("Them".into())
            }
        );
        assert_eq!(
            SpeakerLabels::for_session(LiveMode::Mixed, Channel::Mixed, true, "Me", "Them"),
            SpeakerLabels::Numbered {
                prefix: "Speaker".into(),
                fallback: None
            }
        );
        assert_eq!(
            SpeakerLabels::for_session(LiveMode::Mixed, Channel::Mixed, false, "Me", "Them"),
            SpeakerLabels::None
        );
    }

    #[test]
    fn fragment_sentence_rule_needs_length() {
        let mut b = fragment_builder(SpeakerLabels::Fixed("Me".into()));
        let mut out = Vec::new();
        out.extend(b.on_event(&delta("Hi."), 1.0)); // short sentence: stays open
        assert!(finals(&out).is_empty());
        out.extend(b.on_event(&delta(" How"), 2.0));
        out.extend(b.on_event(&delta(" are"), 3.4));
        out.extend(b.on_event(&delta(" you"), 4.8));
        out.extend(b.on_event(&delta(" doing"), 5.6));
        out.extend(b.on_event(&delta(" today?"), 7.0)); // 6 s line ending with ? → close
        assert_eq!(
            finals(&out),
            [(Some("Me"), "Hi. How are you doing today?", 0.5, 6.5)]
        );
        assert_eq!(
            out.last().unwrap(),
            &LineUpdate::Preview {
                channel: Channel::Mic,
                speaker: Some("Me".into()),
                text: String::new()
            }
        );
        let previews: Vec<_> = out
            .iter()
            .filter_map(|u| match u {
                LineUpdate::Preview { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            previews,
            [
                "Hi.",
                "Hi. How",
                "Hi. How are",
                "Hi. How are you",
                "Hi. How are you doing",
                ""
            ]
        );
    }

    #[test]
    fn fragment_sentence_rule_by_words() {
        let mut b = fragment_builder(SpeakerLabels::None);
        let mut out = Vec::new();
        for i in 0..17 {
            out.extend(b.on_event(&delta(" w"), 1.0 + i as f64 * 0.1));
        }
        out.extend(b.on_event(&delta(" end."), 2.8));
        let f = finals(&out);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].1.split(' ').count(), 18);
        assert_eq!(f[0].0, None);
    }

    #[test]
    fn fragment_idle_and_max_length() {
        let mut b = fragment_builder(SpeakerLabels::None);
        let mut out = b.on_event(&delta("waiting for"), 1.0);
        out.extend(b.tick(2.4));
        assert!(finals(&out).is_empty());
        out.extend(b.tick(2.5));
        assert_eq!(finals(&out), [(None, "waiting for", 0.5, 0.5)]);
        assert!(matches!(out.last().unwrap(), LineUpdate::Preview { text, .. } if text.is_empty()));

        // Idle also closes a line when the next fragment arrives late (no tick in between).
        let mut b = fragment_builder(SpeakerLabels::None);
        let mut out = b.on_event(&delta("one"), 1.0);
        out.extend(b.on_event(&delta(" two"), 5.0));
        assert_eq!(finals(&out), [(None, "one", 0.5, 0.5)]);

        // 30 s cap on continuous unpunctuated speech.
        let mut b = fragment_builder(SpeakerLabels::None);
        let mut out = Vec::new();
        for i in 0..80 {
            out.extend(b.on_event(&delta(" word"), 1.0 + i as f64 * 0.5));
        }
        out.extend(b.finish(41.0));
        let f = finals(&out);
        assert_eq!(f.len(), 2);
        assert!((f[0].3 - f[0].2 - MAX_LINE_SECONDS).abs() < 1e-9, "{f:?}");
        assert_eq!(f.iter().map(|l| l.1.split(' ').count()).sum::<usize>(), 80);
    }

    #[test]
    fn diarized_fragments_split_on_speaker_and_number_by_appearance() {
        let mut b = fragment_builder(SpeakerLabels::Numbered {
            prefix: "Them".into(),
            fallback: Some("Them".into()),
        });
        let mut out = Vec::new();
        out.extend(b.on_event(&diar("speaker_3", "Hi,"), 1.0));
        out.extend(b.on_event(&diar("speaker_3", " there"), 1.5));
        out.extend(b.on_event(&diar("speaker_0", " Hello"), 2.0));
        out.extend(b.on_event(&delta(" and"), 2.5)); // no speaker → fallback label → speaker change
        out.extend(b.on_event(&diar("speaker_3", " bye"), 3.0));
        out.extend(b.finish(4.0));
        assert_eq!(
            finals(&out),
            [
                (Some("Them 1"), "Hi, there", 0.5, 1.0),
                (Some("Them 2"), "Hello", 1.5, 1.5),
                (Some("Them"), "and", 2.0, 2.0),
                (Some("Them 1"), "bye", 2.5, 2.5)
            ]
        );
    }

    #[test]
    fn interim_style_uses_segment_times_and_clock_fallback() {
        let mut cfg = LineBuilderConfig::new(
            Channel::System,
            "deepgram-nova-3",
            SpeakerLabels::Fixed("Them".into()),
        );
        cfg.session_offset = 10.0;
        let mut b = LineBuilder::new(cfg);
        let interim = |t: &str| {
            RealtimeEvent::Interim(TranscriptEvent {
                text: t.into(),
                ..Default::default()
            })
        };
        let committed = |t: &str, span: Option<(f64, f64)>| {
            RealtimeEvent::Committed(TranscriptEvent {
                text: t.into(),
                segments: span
                    .map(|(s, e)| {
                        vec![LiveSegment {
                            start_seconds: Some(s),
                            end_seconds: Some(e),
                            text: t.into(),
                            speaker: None,
                        }]
                    })
                    .unwrap_or_default(),
                ..Default::default()
            })
        };
        let mut out = b.on_event(&interim("Thanks for"), 12.0);
        out.extend(b.on_event(&interim("Thanks for calling"), 13.0));
        out.extend(b.on_event(&committed("Thanks for calling.", Some((0.5, 2.7))), 13.5));
        out.extend(b.on_event(&interim("Next"), 15.0));
        out.extend(b.on_event(&committed("Next one.", None), 16.0));
        out.extend(b.on_event(&interim("dangling"), 17.0));
        out.extend(b.finish(18.0));
        assert_eq!(
            finals(&out),
            [
                (Some("Them"), "Thanks for calling.", 10.5, 12.7),
                (Some("Them"), "Next one.", 14.5, 15.5),
                (Some("Them"), "dangling", 16.5, 17.5)
            ]
        );
        assert!(matches!(out.last().unwrap(), LineUpdate::Preview { text, .. } if text.is_empty()));
        assert!(b.finish(19.0).is_empty(), "finish is idempotent");
        assert!(b.on_event(&interim("late"), 20.0).is_empty());
    }

    #[test]
    fn terminal_event_flushes() {
        let mut b = fragment_builder(SpeakerLabels::None);
        b.on_event(&delta("open line"), 1.0);
        let done = RealtimeEvent::parse(
            r#"{"type":"session.completed","session_id":"rt","result":{"text":"open line"}}"#,
        )
        .unwrap();
        let out = b.on_event(&done, 2.0);
        assert_eq!(finals(&out), [(None, "open line", 0.5, 0.5)]);
    }

    #[test]
    fn clean_completion_drops_uncommitted_interim() {
        let interim = RealtimeEvent::Interim(TranscriptEvent {
            text: "The number four".into(),
            ..Default::default()
        });
        let done = RealtimeEvent::parse(
            r#"{"type":"session.completed","session_id":"rt","result":{"text":"We did."}}"#,
        )
        .unwrap();
        let failed =
            RealtimeEvent::parse(r#"{"type":"session.failed","session_id":"rt","result":null}"#)
                .unwrap();
        let config =
            LineBuilderConfig::new(Channel::System, "deepgram-nova-3", SpeakerLabels::None);

        let mut b = LineBuilder::new(config.clone());
        b.on_event(&interim, 30.0);
        let out = b.on_event(&done, 31.0);
        assert!(finals(&out).is_empty());
        assert!(matches!(out.as_slice(), [LineUpdate::Preview { text, .. }] if text.is_empty()));

        let mut b = LineBuilder::new(config);
        b.on_event(&interim, 30.0);
        let out = b.on_event(&failed, 31.0);
        assert_eq!(finals(&out), [(None, "The number four", 29.5, 30.5)]);
    }
}
