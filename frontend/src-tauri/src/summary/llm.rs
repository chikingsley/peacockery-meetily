//! The one place a summary talks to a language model: `POST /v1/text` on Peacockery Voice.

use peacockery_voice::{Message, Routing, TextContext, TextRequest, VoiceClient};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::voice::{describe_error, VoiceService, VoiceSettings};

/// `/v1/text` accepts at most three fallback models.
const MAX_FALLBACK_MODELS: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryLlmRequest {
    /// Peacockery model id; `None` = `VoiceSettings.summary_model`.
    pub model: Option<String>,
    /// `None` = `VoiceSettings.summary_fallback_models`.
    pub fallback_models: Option<Vec<String>>,
    pub system: String,
    pub user: String,
    pub max_output_tokens: u32,
    /// Stable per meeting (`meetily:<meeting_id>`) so the transcript prefix caches.
    pub conversation_id: String,
    /// Unique per generation (`summary:<meeting_id>:<started_at>`).
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryLlmResponse {
    pub text: String,
    /// Model that actually answered (may be a fallback).
    pub model: String,
    pub provider: String,
    /// `usage` from the API (tokens, cost).
    pub usage: serde_json::Value,
}

/// The wire request: model and fallbacks from the request, else from the settings.
pub fn text_request(settings: &VoiceSettings, req: &SummaryLlmRequest) -> TextRequest {
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(&settings.summary_model)
        .to_string();
    let mut fallbacks: Vec<String> = Vec::new();
    for candidate in req.fallback_models.as_ref().unwrap_or(&settings.summary_fallback_models) {
        let candidate = candidate.trim();
        if !candidate.is_empty() && candidate != model && !fallbacks.iter().any(|f| f == candidate) {
            fallbacks.push(candidate.to_string());
        }
    }
    fallbacks.truncate(MAX_FALLBACK_MODELS);

    let mut request = TextRequest::new(model, vec![Message::system(&req.system), Message::user(&req.user)]);
    request.max_output_tokens = Some(req.max_output_tokens);
    request.context = Some(TextContext {
        conversation_id: Some(req.conversation_id.clone()),
        turn_id: Some(req.turn_id.clone()),
    });
    request.routing = (!fallbacks.is_empty()).then_some(Routing { fallback_models: fallbacks, attempt_timeout_ms: None });
    request
}

/// Generate one completion with an explicit client (no app state).
pub async fn generate_with(
    client: &VoiceClient,
    settings: &VoiceSettings,
    req: &SummaryLlmRequest,
) -> Result<SummaryLlmResponse, String> {
    let request = text_request(settings, req);
    log::info!(
        "Summary request {} to {} (fallbacks {:?})",
        req.turn_id,
        request.model,
        request.routing.as_ref().map(|r| &r.fallback_models)
    );
    let response = client.generate(&request).await.map_err(|e| describe_error(&e))?;
    Ok(SummaryLlmResponse {
        text: response.text,
        model: response.model,
        provider: response.provider.unwrap_or_default(),
        usage: serde_json::to_value(&response.usage).unwrap_or(serde_json::Value::Null),
    })
}

/// Generate one completion for a summary.
pub async fn generate(app: &tauri::AppHandle, req: SummaryLlmRequest) -> Result<SummaryLlmResponse, String> {
    let voice = app.state::<VoiceService>();
    let client = voice.client()?;
    generate_with(&client, &voice.settings(), &req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(model: Option<&str>, fallbacks: Option<Vec<&str>>) -> SummaryLlmRequest {
        SummaryLlmRequest {
            model: model.map(str::to_string),
            fallback_models: fallbacks.map(|f| f.into_iter().map(str::to_string).collect()),
            system: "Summarize.".into(),
            user: "Me: hello".into(),
            max_output_tokens: 4096,
            conversation_id: "meetily:m1".into(),
            turn_id: "summary:m1:t".into(),
        }
    }

    #[test]
    fn settings_fill_model_and_fallbacks() {
        let settings = VoiceSettings::default();
        let wire = serde_json::to_value(text_request(&settings, &request(None, None))).unwrap();
        assert_eq!(wire["model"], "google-gemini-3.8-flash");
        assert_eq!(wire["maxOutputTokens"], 4096);
        assert_eq!(wire["context"]["conversationId"], "meetily:m1");
        assert_eq!(wire["context"]["turnId"], "summary:m1:t");
        assert_eq!(
            wire["routing"]["fallbackModels"],
            serde_json::json!(["deepseek-deepseek-v4-flash", "google-gemini-3.1-flash-lite"])
        );
        assert_eq!(wire["messages"][0]["role"], "system");
        assert_eq!(wire["messages"][1]["content"], "Me: hello");
    }

    #[test]
    fn explicit_model_and_fallbacks_win_and_are_cleaned() {
        let settings = VoiceSettings::default();
        let r = text_request(
            &settings,
            &request(Some("anthropic-claude-sonnet-5-5"), Some(vec!["anthropic-claude-sonnet-5-5", "a", "a", "b", "c", "d"])),
        );
        assert_eq!(r.model, "anthropic-claude-sonnet-5-5");
        assert_eq!(r.routing.unwrap().fallback_models, vec!["a", "b", "c"]);
        let none = text_request(&settings, &request(Some(" "), Some(vec![])));
        assert_eq!(none.model, "google-gemini-3.8-flash");
        assert!(none.routing.is_none());
    }
}
