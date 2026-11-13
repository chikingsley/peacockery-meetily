//! `GET /v1/models` and the curated model allow-lists the app offers (ARCHITECTURE.md §2).

use serde::{Deserialize, Serialize};

use crate::client::VoiceClient;
use crate::error::Result;

/// `GET /v1/models` response. Unknown fields are ignored; missing optional ones default.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    #[serde(default)]
    pub models: Vec<CatalogModel>,
    #[serde(default)]
    pub presentation_schema_version: Option<u32>,
}

/// One catalog entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    /// `transcription` or `language`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub upstream_model: Option<String>,
    #[serde(default)]
    pub routes: Routes,
    #[serde(default)]
    pub reasoning: Option<Reasoning>,
}

/// Transcription routes of a model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Routes {
    #[serde(default)]
    pub batch: Option<Route>,
    #[serde(default)]
    pub realtime: Option<Route>,
}

/// One transcription route.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Route {
    /// Model id to send for this route (the realtime `model` query parameter).
    pub model: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub upstream_model: Option<String>,
    #[serde(default)]
    pub supported_languages: Vec<String>,
    #[serde(default)]
    pub supports_automatic_language: bool,
    #[serde(default)]
    pub supports_diarization: bool,
}

/// Reasoning efforts a language model accepts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Reasoning {
    #[serde(default)]
    pub default_effort: Option<String>,
    #[serde(default)]
    pub supported_efforts: Vec<String>,
}

impl Catalog {
    /// Model by public id.
    pub fn get(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// WebSocket `model` value for a public id (`routes.realtime.model`), if it has a realtime route.
    pub fn realtime_model_id(&self, public_id: &str) -> Option<&str> {
        self.get(public_id)?
            .routes
            .realtime
            .as_ref()
            .map(|r| r.model.as_str())
    }


}

impl VoiceClient {
    /// `GET /v1/models` (needs `models:read`).
    pub async fn models(&self) -> Result<Catalog> {
        self.get_json("v1/models").await
    }
}

/// A model as the settings UI shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub note: String,
    /// Present in the catalog with the route this use needs.
    pub available: bool,
    pub supports_diarization: bool,
    pub recommended: bool,
}

struct Curated {
    id: &'static str,
    label: &'static str,
    note: &'static str,
    recommended: bool,
}

const LIVE: &[Curated] = &[
    Curated { id: "elevenlabs-scribe_v3", label: "Scribe v3 Live (via Superwhisper)", note: "default; live speaker labels, Nova fallback", recommended: true },
    Curated {
        id: "deepgram-nova-3",
        label: "Deepgram Nova (via Superwhisper)",
        note: "fallback; low-latency live transcript",
        recommended: false,
    },
    Curated {
        id: "mistral-voxtral-mini-transcribe-realtime-3",
        label: "Voxtral Mini Realtime 3",
        note: "more accurate, live speaker labels",
        recommended: false,
    },
    Curated {
        id: "elevenlabs-scribe_v2",
        label: "Scribe v2 Realtime (via Superwhisper)",
        note: "accurate, less reliable",
        recommended: false,
    },
    Curated {
        id: "cohere-transcribe",
        label: "Cohere Transcribe (self-hosted on GMK)",
        note: "free, may be offline",
        recommended: false,
    },
];

const FINAL: &[Curated] = &[
    Curated { id: "elevenlabs-scribe_v3", label: "Scribe v3", note: "optional final transcript", recommended: true },
    Curated {
        id: "elevenlabs-scribe_v2",
        label: "Scribe v2",
        note: "diarization + word timestamps",
        recommended: false,
    },
    Curated {
        id: "mistral-voxtral-small-transcribe-3",
        label: "Voxtral Small Transcribe 3",
        note: "top overall WER; segment timestamps",
        recommended: false,
    },
    Curated {
        id: "mistral-voxtral-mini-transcribe-3",
        label: "Voxtral Mini Transcribe 3",
        note: "",
        recommended: false,
    },
    Curated {
        id: "deepgram-nova-3",
        label: "Deepgram Nova (via Superwhisper)",
        note: "",
        recommended: false,
    },
    Curated {
        id: "cohere-transcribe",
        label: "Cohere Transcribe (GMK)",
        note: "free, no diarization, may be offline",
        recommended: false,
    },
];

const SUMMARY: &[Curated] = &[
    Curated {
        id: "google-gemini-3.8-flash",
        label: "Gemini 3.8 Flash",
        note: "default, ~5 s on a 1-hour call",
        recommended: true,
    },
    Curated {
        id: "anthropic-claude-sonnet-5-5",
        label: "Claude Sonnet 5.5",
        note: "best notes; near the API's 10 s cap on long calls",
        recommended: false,
    },
    Curated {
        id: "deepseek-deepseek-v4-flash",
        label: "DeepSeek V4 Flash",
        note: "",
        recommended: false,
    },
    Curated {
        id: "google-gemini-3.1-flash-lite",
        label: "Gemini 3.1 Flash Lite",
        note: "fastest",
        recommended: false,
    },
    Curated {
        id: "openai-gpt-5.4-mini",
        label: "GPT-5.4 mini",
        note: "",
        recommended: false,
    },
];

fn options(
    list: &[Curated],
    route: impl Fn(&CatalogModel) -> Option<(bool, bool)>,
    catalog: &Catalog,
) -> Vec<ModelOption> {
    list.iter()
        .map(|c| {
            let (available, supports_diarization) =
                catalog.get(c.id).and_then(&route).unwrap_or((false, false));
            ModelOption {
                id: c.id.into(),
                label: c.label.into(),
                note: c.note.into(),
                available,
                supports_diarization,
                recommended: c.recommended,
            }
        })
        .collect()
}

/// Live (realtime) models; available = the catalog lists a realtime route.
pub fn curated_live(catalog: &Catalog) -> Vec<ModelOption> {
    options(
        LIVE,
        |m| {
            m.routes
                .realtime
                .as_ref()
                .map(|r| (true, r.supports_diarization))
        },
        catalog,
    )
}

/// Final (batch) models; available = the catalog lists a batch route.
pub fn curated_final(catalog: &Catalog) -> Vec<ModelOption> {
    options(
        FINAL,
        |m| {
            m.routes
                .batch
                .as_ref()
                .map(|r| (true, r.supports_diarization))
        },
        catalog,
    )
}

/// Summary (language) models; available = the catalog lists the model as a language model.
pub fn curated_summary(catalog: &Catalog) -> Vec<ModelOption> {
    options(
        SUMMARY,
        |m| (m.kind == "language").then_some((true, false)),
        catalog,
    )
}

/// Provider family of a transcription model id, which decides which options and event style apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    /// `mistral-voxtral-mini-transcribe-realtime-3`: the only route with context_bias / turn_detection / delay.
    VoxtralRealtime3,
    /// Other Mistral models (Voxtral 3 batch, voxtral-mini-latest / realtime-2602).
    Mistral,
    Deepgram,
    /// ElevenLabs Scribe.
    Scribe,
    Cohere,
    Other,
}

impl ModelFamily {
    /// Family from a public or route model id.
    pub fn of(model_id: &str) -> Self {
        if model_id.starts_with("mistral-voxtral-mini-transcribe-realtime-3") {
            ModelFamily::VoxtralRealtime3
        } else if model_id.starts_with("mistral-") {
            ModelFamily::Mistral
        } else if model_id.starts_with("deepgram-") {
            ModelFamily::Deepgram
        } else if model_id.starts_with("elevenlabs-") {
            ModelFamily::Scribe
        } else if model_id.starts_with("cohere-") {
            ModelFamily::Cohere
        } else {
            ModelFamily::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Catalog {
        let mut catalog: Catalog = serde_json::from_str(include_str!("../tests/fixtures/models.json")).unwrap();
        let mut v3 = catalog.models.iter().find(|m| m.id == "elevenlabs-scribe_v2").unwrap().clone();
        v3.id = "elevenlabs-scribe_v3".into();
        if let Some(route) = &mut v3.routes.realtime { route.supports_diarization = true; }
        catalog.models.push(v3);
        catalog
    }

    #[test]
    fn parses_live_catalog() {
        let catalog = fixture();
        assert_eq!(catalog.models.len(), 52);
        assert_eq!(catalog.presentation_schema_version, Some(1));
        assert_eq!(
            catalog.realtime_model_id("mistral-voxtral-mini-latest"),
            Some("mistral-voxtral-mini-transcribe-realtime-2602")
        );
        assert_eq!(
            catalog.realtime_model_id("elevenlabs-scribe_v2"),
            Some("elevenlabs-scribe_v2")
        );
        assert_eq!(
            catalog.realtime_model_id("mistral-voxtral-small-transcribe-3"),
            None
        );

        assert_eq!(catalog.realtime_model_id("nope"), None);
        let gemini = catalog.get("google-gemini-3.8-flash").unwrap();
        assert_eq!(gemini.kind, "language");
        assert_eq!(
            gemini.reasoning.as_ref().unwrap().supported_efforts,
            ["none"]
        );
    }

    #[test]
    fn tolerant_parsing() {
        let catalog: Catalog =
            serde_json::from_str(r#"{"models":[{"id":"x","future":1,"routes":{"realtime":{"model":"x-rt","new":true}}}]}"#).unwrap();
        assert_eq!(catalog.realtime_model_id("x"), Some("x-rt"));
        assert!(
            !catalog.models[0]
                .routes
                .realtime
                .as_ref()
                .unwrap()
                .supports_diarization
        );
        let empty: Catalog = serde_json::from_str("{}").unwrap();
        assert!(empty.models.is_empty());
    }

    #[test]
    fn curated_lists_against_live_catalog() {
        let catalog = fixture();
        let live = curated_live(&catalog);
        assert_eq!(live.len(), 5);
        assert!(live.iter().all(|m| m.available), "{live:?}");
        let nova = &live[0];
        assert!(nova.recommended && nova.id == "elevenlabs-scribe_v3");
        assert_eq!(live.iter().filter(|m| m.recommended).count(), 1);
        let vox = live
            .iter()
            .find(|m| m.id == "mistral-voxtral-mini-transcribe-realtime-3")
            .unwrap();
        assert!(!vox.recommended && vox.supports_diarization);
        let scribe = live
            .iter()
            .find(|m| m.id == "elevenlabs-scribe_v2")
            .unwrap();
        assert!(!scribe.supports_diarization);

        let fin = curated_final(&catalog);
        assert!(fin.iter().all(|m| m.available), "{fin:?}");
        assert!(
            fin[0].recommended
                && fin[0].supports_diarization
                && fin[0].id == "elevenlabs-scribe_v3"
        );
        assert!(
            !fin.iter()
                .find(|m| m.id == "cohere-transcribe")
                .unwrap()
                .supports_diarization
        );

        let summary = curated_summary(&catalog);
        assert_eq!(summary.len(), 5);
        assert!(
            summary
                .iter()
                .all(|m| m.available && !m.supports_diarization),
            "{summary:?}"
        );
        assert_eq!(summary.iter().filter(|m| m.recommended).count(), 1);
    }

    #[test]
    fn curated_lists_mark_missing_models_unavailable() {
        let live = curated_live(&Catalog::default());
        assert!(live.iter().all(|m| !m.available && !m.supports_diarization));
        // A batch-only entry is not a live option.
        let catalog: Catalog =
            serde_json::from_str(r#"{"models":[{"id":"deepgram-nova-3","kind":"transcription","routes":{"batch":{"model":"deepgram-nova-3","supports_diarization":true}}}]}"#)
                .unwrap();
        assert!(!curated_live(&catalog).iter().any(|m| m.available));
        assert!(
            curated_final(&catalog)
                .iter()
                .find(|m| m.id == "deepgram-nova-3")
                .unwrap()
                .available
        );
        // A transcription model with a summary id is not a summary option.
        let catalog: Catalog = serde_json::from_str(
            r#"{"models":[{"id":"openai-gpt-5.4-mini","kind":"transcription"}]}"#,
        )
        .unwrap();
        assert!(!curated_summary(&catalog).iter().any(|m| m.available));
    }

    #[test]
    fn families() {
        assert_eq!(
            ModelFamily::of("mistral-voxtral-mini-transcribe-realtime-3"),
            ModelFamily::VoxtralRealtime3
        );
        assert_eq!(
            ModelFamily::of("mistral-voxtral-mini-transcribe-realtime-2602"),
            ModelFamily::Mistral
        );
        assert_eq!(
            ModelFamily::of("mistral-voxtral-small-transcribe-3"),
            ModelFamily::Mistral
        );
        assert_eq!(ModelFamily::of("deepgram-nova-3"), ModelFamily::Deepgram);
        assert_eq!(ModelFamily::of("elevenlabs-scribe_v2"), ModelFamily::Scribe);
        assert_eq!(ModelFamily::of("cohere-transcribe"), ModelFamily::Cohere);
        assert_eq!(ModelFamily::of("azure-speech"), ModelFamily::Other);
    }
}
