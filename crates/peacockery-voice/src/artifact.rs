//! TranscriptionArtifact (schema_version 2) subset, with the raw JSON kept for saving.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A transcription result from a batch job (`JobView.result`) or a realtime terminal event.
///
/// Only the fields this app reads are typed; [`Artifact::raw`] keeps the full document
/// (provider capture, warnings, tokens …) and is what `Serialize` writes back out.
#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    pub text: String,
    pub content: Content,
    pub language: Option<LanguageInfo>,
    pub metrics: Option<Metrics>,
    pub provenance: Option<Provenance>,
    pub raw: Value,
}

#[derive(Deserialize)]
struct Typed {
    #[serde(default)]
    text: String,
    #[serde(default)]
    content: Content,
    #[serde(default)]
    language: Option<LanguageInfo>,
    #[serde(default)]
    metrics: Option<Metrics>,
    #[serde(default)]
    provenance: Option<Provenance>,
}

impl Artifact {
    /// Parse from a JSON value, keeping it as [`Artifact::raw`].
    pub fn from_value(raw: Value) -> Result<Self, serde_json::Error> {
        let typed = Typed::deserialize(&raw)?;
        Ok(Self {
            text: typed.text,
            content: typed.content,
            language: typed.language,
            metrics: typed.metrics,
            provenance: typed.provenance,
            raw,
        })
    }

    /// Audio duration reported by the provider, if any.
    pub fn duration_seconds(&self) -> Option<f64> {
        self.metrics.as_ref().and_then(|m| m.audio_duration_seconds)
    }
}

impl<'de> Deserialize<'de> for Artifact {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Artifact::from_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Artifact {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.raw.serialize(serializer)
    }
}

/// `content` tracks used for turns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Content {
    #[serde(default)]
    pub words: Track,
    #[serde(default)]
    pub segments: Track,
    #[serde(default)]
    pub speaker_turns: Track,
}

/// One content track: availability (`available`, `not_requested`, `provider_omitted`, `unsupported`) and items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub items: Vec<TimedText>,
}

/// A word, segment or speaker turn. Times may be null (Voxtral realtime diarized segments).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimedText {
    #[serde(default)]
    pub start_seconds: Option<f64>,
    #[serde(default)]
    pub end_seconds: Option<f64>,
    #[serde(default)]
    pub text: String,
    /// Normalized provider speaker: strings kept as-is (`speaker_0`), numbers become `speaker_<n>`.
    #[serde(default, deserialize_with = "de_speaker")]
    pub speaker: Option<String>,
}

/// `language` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LanguageInfo {
    #[serde(default)]
    pub requested: Option<String>,
    #[serde(default)]
    pub detected: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// `metrics` subset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    #[serde(default)]
    pub audio_duration_seconds: Option<f64>,
    #[serde(default)]
    pub provider_latency_ms: Option<f64>,
    #[serde(default)]
    pub queue_delay_ms: Option<f64>,
    #[serde(default)]
    pub wall_latency_ms: Option<f64>,
    #[serde(default)]
    pub first_result_latency_ms: Option<f64>,
    #[serde(default)]
    pub realtime_speed_factor: Option<f64>,
}

/// `provenance` subset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub upstream_model: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub provider_request_id: Option<String>,
}

/// Speaker that may arrive as `"speaker_0"`, `0` or `null`.
pub(crate) fn de_speaker<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s),
        Some(Value::Number(n)) => Some(format!("speaker_{n}")),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_result(name: &str) -> Artifact {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let job: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        Artifact::from_value(job["result"].clone()).unwrap()
    }

    #[test]
    fn parses_scribe_batch() {
        let a = job_result("b-scribe.json");
        assert_eq!(a.content.words.items.len(), 188);
        assert_eq!(a.content.words.availability.as_deref(), Some("available"));
        assert_eq!(
            a.content.speaker_turns.availability.as_deref(),
            Some("provider_omitted")
        );
        assert!(a.content.speaker_turns.items.is_empty());
        let w = &a.content.words.items[0];
        assert_eq!(
            (w.start_seconds, w.end_seconds, w.text.as_str()),
            (Some(0.06), Some(0.36), "Hi,")
        );
        assert_eq!(w.speaker.as_deref(), Some("speaker_0"));
        assert_eq!(
            a.language.as_ref().unwrap().detected.as_deref(),
            Some("eng")
        );
        assert_eq!(
            a.provenance.as_ref().unwrap().model.as_deref(),
            Some("elevenlabs-scribe_v2")
        );
        assert!((a.duration_seconds().unwrap() - 77.595125).abs() < 1e-9);
        assert!(a.text.starts_with("Hi, thanks"));
    }

    #[test]
    fn parses_voxtral_batch_and_round_trips_raw() {
        let a = job_result("b-voxsmall.json");
        assert_eq!(a.content.speaker_turns.items.len(), 27);
        assert_eq!(a.content.segments.items.len(), 27);
        assert!(a.content.words.items.is_empty());
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back, a.raw);
        assert!(back["provider_capture"].is_object());
    }

    #[test]
    fn speaker_forms_and_nulls() {
        let t: Track = serde_json::from_str(
            r#"{"items":[{"start_seconds":null,"end_seconds":null,"text":"a","speaker":3},
                          {"text":"b","speaker":"speaker_1"},{"text":"c","speaker":null},{"text":"d","speaker":""},{"text":"e"}]}"#,
        )
        .unwrap();
        let speakers: Vec<_> = t.items.iter().map(|i| i.speaker.as_deref()).collect();
        assert_eq!(
            speakers,
            [Some("speaker_3"), Some("speaker_1"), None, None, None]
        );
        assert_eq!(t.items[0].start_seconds, None);
    }

    #[test]
    fn minimal_artifact() {
        let a: Artifact = serde_json::from_str(r#"{"text":"hello"}"#).unwrap();
        assert_eq!(a.text, "hello");
        assert!(a.content.words.items.is_empty() && a.metrics.is_none());
    }
}
