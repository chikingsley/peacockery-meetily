//! Voice settings commands (ARCHITECTURE.md §8 "Voice settings").

use std::time::Instant;

use peacockery_voice::{curated_final, curated_live, curated_summary, Catalog, ModelOption};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::credential::{self, KeyStatus};
use super::service::{describe_error, VoiceService};
use super::settings::{self, VoiceSettings};

#[derive(Debug, Serialize)]
pub struct ConnectionTest {
    pub ok: bool,
    pub environment: Option<String>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Curated model lists. When the catalog cannot be read, every option is
/// `available: false` and `error` says why.
#[derive(Debug, Serialize)]
pub struct ModelLists {
    pub live: Vec<ModelOption>,
    #[serde(rename = "final")]
    pub final_models: Vec<ModelOption>,
    pub summary: Vec<ModelOption>,
    pub error: Option<String>,
}

impl ModelLists {
    fn from_catalog(catalog: &Catalog, error: Option<String>) -> Self {
        Self {
            live: curated_live(catalog),
            final_models: curated_final(catalog),
            summary: curated_summary(catalog),
            error,
        }
    }
}

#[tauri::command]
pub async fn voice_get_settings(voice: State<'_, VoiceService>) -> Result<VoiceSettings, String> {
    Ok(voice.settings())
}

#[tauri::command]
pub async fn voice_save_settings(
    app: AppHandle,
    voice: State<'_, VoiceService>,
    settings: VoiceSettings,
) -> Result<VoiceSettings, String> {
    let saved = settings::save(&app, settings)?;
    voice.set_settings(saved.clone());
    log::info!(
        "Voice settings saved: live {} ({:?}, enabled {}), final {}, summary {}",
        saved.live_model,
        saved.live_mode,
        saved.live_enabled,
        saved.final_model,
        saved.summary_model
    );
    let _ = app.emit("voice-settings-updated", &saved);
    Ok(saved)
}

#[tauri::command]
pub async fn voice_get_key_status() -> Result<KeyStatus, String> {
    Ok(KeyStatus::of(credential::read_key()?.as_deref()))
}

#[tauri::command]
pub async fn voice_set_api_key(voice: State<'_, VoiceService>, key: String) -> Result<KeyStatus, String> {
    let status = credential::store_key(&key)?;
    voice.invalidate();
    Ok(status)
}

#[tauri::command]
pub async fn voice_clear_api_key(voice: State<'_, VoiceService>) -> Result<KeyStatus, String> {
    credential::clear_key()?;
    voice.invalidate();
    Ok(KeyStatus::of(None))
}

#[tauri::command]
pub async fn voice_test_connection(voice: State<'_, VoiceService>) -> Result<ConnectionTest, String> {
    let started = Instant::now();
    let result = match voice.client() {
        Ok(client) => client.health().await.map_err(|e| describe_error(&e)),
        Err(e) => Err(e),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    Ok(match result {
        Ok(health) => ConnectionTest {
            ok: health.ok,
            environment: health.environment,
            latency_ms,
            error: (!health.ok).then(|| "The server reported it is not healthy".to_string()),
        },
        Err(error) => ConnectionTest { ok: false, environment: None, latency_ms, error: Some(error) },
    })
}

#[tauri::command]
pub async fn voice_list_models(voice: State<'_, VoiceService>) -> Result<ModelLists, String> {
    Ok(match voice.catalog().await {
        Ok(catalog) => ModelLists::from_catalog(&catalog, None),
        Err(error) => {
            log::warn!("Model catalog unavailable: {}", error);
            ModelLists::from_catalog(&Catalog::default(), Some(error))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_catalog_still_lists_curated_models() {
        let lists = ModelLists::from_catalog(&Catalog::default(), Some("offline".into()));
        assert_eq!(lists.live.len(), 5);
        assert_eq!(lists.final_models.len(), 6);
        assert_eq!(lists.summary.len(), 5);
        assert!(lists.live.iter().chain(&lists.final_models).all(|o| !o.available));
        let json = serde_json::to_value(&lists).unwrap();
        assert!(json.get("final").is_some());
        assert_eq!(json["error"], "offline");
    }

    #[test]
    fn live_catalog_marks_availability() {
        let catalog: Catalog = serde_json::from_str(include_str!(
            "../../../../crates/peacockery-voice/tests/fixtures/models.json"
        ))
        .unwrap();
        let lists = ModelLists::from_catalog(&catalog, None);
        let scribe = lists.final_models.iter().find(|o| o.id == "elevenlabs-scribe_v2").unwrap();
        assert!(scribe.available && scribe.supports_diarization && !scribe.recommended);
    }
}
