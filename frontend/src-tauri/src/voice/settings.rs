//! `VoiceSettings` (ARCHITECTURE.md §2), stored by Rust in `voice-settings.json`.

use peacockery_voice::{curated_final, curated_live, curated_summary, Catalog, DEFAULT_BASE_URL};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

pub const STORE_FILE: &str = "voice-settings.json";
const SETTINGS_KEY: &str = "settings";
const INSTALLATION_ID_KEY: &str = "installation_id";
/// `/v1/text` accepts at most three fallback models.
const MAX_FALLBACK_MODELS: usize = 3;

/// When the final transcript runs after a recording stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoFinal {
    Always,
    /// Unless the live transcript completed on the final model.
    WhenLiveIncomplete,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub base_url: String,
    pub live_enabled: bool,
    #[serde(default = "fallback_enabled")]
    pub nova_fallback: bool,
    pub live_model: String,
    pub final_model: String,
    pub final_diarize: bool,
    pub auto_final: AutoFinal,
    /// ISO-639-1, or `auto`.
    pub language: String,
    pub keyterms: Vec<String>,
    pub transcript_corrections: Vec<super::corrections::TranscriptCorrection>,
    pub me_label: String,
    pub them_label: String,
    pub summary_model: String,
    pub summary_fallback_models: Vec<String>,
    pub auto_summary: bool,
}

fn fallback_enabled() -> bool {
    true
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            live_enabled: true,
            nova_fallback: true,
            live_model: "elevenlabs-scribe_v3".to_string(),
            final_model: "elevenlabs-scribe_v3".to_string(),
            final_diarize: true,
            auto_final: AutoFinal::WhenLiveIncomplete,
            language: "en".to_string(),
            keyterms: Vec::new(),
            transcript_corrections: Vec::new(),
            me_label: "Me".to_string(),
            them_label: "Them".to_string(),
            summary_model: "google-gemini-3.8-flash".to_string(),
            summary_fallback_models: vec![
                "deepseek-deepseek-v4-flash".to_string(),
                "google-gemini-3.1-flash-lite".to_string(),
            ],
            auto_summary: false,
        }
    }
}

fn allowed(list: Vec<peacockery_voice::ModelOption>, id: &str) -> bool {
    list.iter().any(|option| option.id == id)
}

pub fn is_live_model(id: &str) -> bool {
    allowed(curated_live(&Catalog::default()), id)
}

pub fn is_final_model(id: &str) -> bool {
    allowed(curated_final(&Catalog::default()), id)
}

pub fn is_summary_model(id: &str) -> bool {
    allowed(curated_summary(&Catalog::default()), id)
}

fn is_language(value: &str) -> bool {
    value == "auto" || (value.len() == 2 && value.chars().all(|c| c.is_ascii_lowercase()))
}

impl VoiceSettings {
    /// Trim text fields, drop empty and duplicate keyterms and fallbacks, then check every
    /// value. Returns the cleaned settings or a message naming the bad field.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        self.live_model = self.live_model.trim().to_string();
        self.final_model = self.final_model.trim().to_string();
        self.summary_model = self.summary_model.trim().to_string();
        self.language = self.language.trim().to_ascii_lowercase();
        self.me_label = self.me_label.trim().to_string();
        self.them_label = self.them_label.trim().to_string();
        self.keyterms = dedup_trimmed(self.keyterms);
        let summary_model = self.summary_model.clone();
        self.summary_fallback_models = dedup_trimmed(self.summary_fallback_models)
            .into_iter()
            .filter(|m| *m != summary_model)
            .collect();

        match url::Url::parse(&self.base_url) {
            Ok(url) if matches!(url.scheme(), "https" | "http") && url.host().is_some() => {}
            _ => {
                return Err(format!(
                    "base_url must be an http(s) URL, got {:?}",
                    self.base_url
                ))
            }
        }
        if !is_live_model(&self.live_model) {
            return Err(format!("Unknown live model {:?}", self.live_model));
        }
        if !is_final_model(&self.final_model) {
            return Err(format!(
                "Unknown final transcript model {:?}",
                self.final_model
            ));
        }
        if !is_summary_model(&self.summary_model) {
            return Err(format!("Unknown summary model {:?}", self.summary_model));
        }
        if let Some(bad) = self
            .summary_fallback_models
            .iter()
            .find(|m| !is_summary_model(m))
        {
            return Err(format!("Unknown summary fallback model {:?}", bad));
        }
        if self.summary_fallback_models.len() > MAX_FALLBACK_MODELS {
            return Err(format!(
                "At most {} summary fallback models",
                MAX_FALLBACK_MODELS
            ));
        }
        if !is_language(&self.language) {
            return Err(format!(
                "language must be a two-letter ISO-639-1 code or \"auto\", got {:?}",
                self.language
            ));
        }
        if self.me_label.is_empty() || self.them_label.is_empty() {
            return Err("Speaker labels cannot be empty".to_string());
        }
        if self.me_label == self.them_label {
            return Err("The \"me\" and \"them\" labels must differ".to_string());
        }
        Ok(self)
    }

    /// Settings from a stored JSON object: every key that is missing, unknown or invalid
    /// keeps its default, so one bad value never resets the rest.
    pub fn from_stored(stored: Option<&Value>) -> Self {
        let defaults = Self::default();
        let Some(Value::Object(stored)) = stored else {
            return defaults;
        };
        let mut merged = match serde_json::to_value(&defaults) {
            Ok(Value::Object(map)) => map,
            _ => return defaults,
        };
        let mut current = defaults;
        for (key, value) in stored {
            if !merged.contains_key(key) {
                log::warn!("Ignoring unknown voice setting {:?}", key);
                continue;
            }
            let mut candidate: Map<String, Value> = merged.clone();
            candidate.insert(key.clone(), value.clone());
            match serde_json::from_value::<Self>(Value::Object(candidate.clone()))
                .map_err(|e| e.to_string())
                .and_then(Self::normalized)
            {
                Ok(settings) => {
                    merged = candidate;
                    current = settings;
                }
                Err(e) => log::warn!("Ignoring stored voice setting {:?}: {}", key, e),
            }
        }
        current
    }
}

fn dedup_trimmed(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if !value.is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// Stored settings with defaults filled in.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> VoiceSettings {
    match app.store(STORE_FILE) {
        Ok(store) => VoiceSettings::from_stored(store.get(SETTINGS_KEY).as_ref()),
        Err(e) => {
            log::warn!("Failed to open {}: {}", STORE_FILE, e);
            VoiceSettings::default()
        }
    }
}

/// Validate and persist; returns the normalized settings.
pub fn save<R: Runtime>(
    app: &AppHandle<R>,
    settings: VoiceSettings,
) -> Result<VoiceSettings, String> {
    let settings = settings.normalized()?;
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("Failed to open {}: {}", STORE_FILE, e))?;
    store.set(
        SETTINGS_KEY,
        serde_json::to_value(&settings).map_err(|e| e.to_string())?,
    );
    store
        .save()
        .map_err(|e| format!("Failed to save voice settings: {}", e))?;
    Ok(settings)
}

/// The persisted installation id (`X-Peacockery-Installation-ID`), created on first use.
pub fn installation_id<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let store = match app.store(STORE_FILE) {
        Ok(store) => store,
        Err(e) => {
            log::warn!("Failed to open {}: {}", STORE_FILE, e);
            return None;
        }
    };
    if let Some(id) = store
        .get(INSTALLATION_ID_KEY)
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
    {
        return Some(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    store.set(INSTALLATION_ID_KEY, Value::String(id.clone()));
    if let Err(e) = store.save() {
        log::warn!("Failed to save the installation id: {}", e);
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_contract_and_are_valid() {
        let defaults = VoiceSettings::default();
        assert_eq!(defaults.clone().normalized().unwrap(), defaults);
        let value = serde_json::to_value(&defaults).unwrap();
        assert_eq!(value["auto_final"], "when_live_incomplete");
        assert_eq!(value["base_url"], "https://voice-lab.peacockery.studio");
        assert_eq!(
            value["summary_fallback_models"],
            json!(["deepseek-deepseek-v4-flash", "google-gemini-3.1-flash-lite"])
        );
        assert_eq!(value["nova_fallback"], true);
        assert_eq!(value.as_object().unwrap().len(), 15);
    }

    #[test]
    fn missing_and_invalid_keys_keep_defaults() {
        let stored = json!({
            "live_enabled": false,
            "final_model": "not-a-model",
            "language": "EN-us",
            "auto_summary": true,
            "unknown": 1,
            "me_label": "  Simon "
        });
        let settings = VoiceSettings::from_stored(Some(&stored));
        assert!(!settings.live_enabled);
        assert_eq!(settings.final_model, "elevenlabs-scribe_v3");
        assert_eq!(settings.language, "en");
        assert!(settings.auto_summary);
        assert_eq!(settings.me_label, "Simon");
        assert_eq!(VoiceSettings::from_stored(None), VoiceSettings::default());
        assert_eq!(
            VoiceSettings::from_stored(Some(&json!("junk"))),
            VoiceSettings::default()
        );
    }

    #[test]
    fn removed_live_topology_keys_are_ignored() {
        let settings = VoiceSettings::from_stored(Some(&json!({
            "live_mode": "split",
            "live_diarize": true,
            "auto_summary": true
        })));
        assert!(settings.auto_summary);
        let value = serde_json::to_value(&settings).unwrap();
        assert!(value.get("live_mode").is_none());
        assert!(value.get("live_diarize").is_none());
    }

    #[test]
    fn transcript_corrections_are_required_on_save() {
        let mut value = serde_json::to_value(VoiceSettings::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("transcript_corrections");
        assert!(serde_json::from_value::<VoiceSettings>(value.clone()).is_err());
        assert_eq!(
            VoiceSettings::from_stored(Some(&value)),
            VoiceSettings::default()
        );
    }

    #[test]
    fn auto_final_loads_by_name_and_a_stored_boolean_keeps_the_default() {
        for (stored, expected) in [
            (json!("always"), AutoFinal::Always),
            (json!("never"), AutoFinal::Never),
            (json!(true), AutoFinal::WhenLiveIncomplete),
            (json!(false), AutoFinal::WhenLiveIncomplete),
        ] {
            let settings = VoiceSettings::from_stored(Some(&json!({
                "auto_final": stored,
                "auto_summary": true
            })));
            assert_eq!(settings.auto_final, expected, "{stored}");
            assert!(settings.auto_summary);
        }
    }

    #[test]
    fn removed_scribe_v2_loads_as_the_default_model() {
        let settings = VoiceSettings::from_stored(Some(&json!({
            "live_model": "elevenlabs-scribe_v2",
            "final_model": "elevenlabs-scribe_v2"
        })));
        assert_eq!(settings.live_model, "elevenlabs-scribe_v3");
        assert_eq!(settings.final_model, "elevenlabs-scribe_v3");
        assert!(!is_live_model("elevenlabs-scribe_v2"));
        assert!(!is_final_model("elevenlabs-scribe_v2"));
    }

    #[test]
    fn normalization_cleans_and_validates() {
        let mut s = VoiceSettings {
            base_url: " https://voice-lab.peacockery.studio/ ".into(),
            keyterms: vec![
                " Cica-Huntek ".into(),
                "".into(),
                "Cica-Huntek".into(),
                "Netask".into(),
            ],
            summary_fallback_models: vec![
                "google-gemini-3.8-flash".into(),
                "openai-gpt-5.4-mini".into(),
            ],
            language: "AUTO".into(),
            ..VoiceSettings::default()
        };
        s = s.normalized().unwrap();
        assert_eq!(s.base_url, "https://voice-lab.peacockery.studio");
        assert_eq!(s.keyterms, vec!["Cica-Huntek", "Netask"]);
        assert_eq!(s.summary_fallback_models, vec!["openai-gpt-5.4-mini"]);
        assert_eq!(s.language, "auto");

        let reject = |f: fn(&mut VoiceSettings)| {
            let mut s = VoiceSettings::default();
            f(&mut s);
            s.normalized().is_err()
        };
        assert!(reject(|s| s.base_url = "ftp://x".into()));
        assert!(reject(|s| s.base_url = "not a url".into()));
        assert!(reject(|s| s.live_model = "elevenlabs-scribe_v1".into()));
        assert!(reject(
            |s| s.final_model = "mistral-voxtral-mini-transcribe-realtime-3".into()
        ));
        assert!(reject(|s| s.summary_model = "elevenlabs-scribe_v3".into()));
        assert!(reject(|s| s.summary_fallback_models = vec!["nope".into()]));
        assert!(reject(|s| {
            s.summary_fallback_models = vec![
                "anthropic-claude-sonnet-5-5".into(),
                "deepseek-deepseek-v4-flash".into(),
                "google-gemini-3.1-flash-lite".into(),
                "openai-gpt-5.4-mini".into(),
            ]
        }));
        assert!(reject(|s| s.language = "english".into()));
        assert!(reject(|s| s.me_label = " ".into()));
        assert!(reject(|s| s.them_label = "Me".into()));
    }
}
