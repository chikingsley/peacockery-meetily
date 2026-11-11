//! `POST /v1/text`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{decode, VoiceClient};
use crate::error::{Error, Result};

/// The server caps each attempt at 10 s; this covers the primary plus 3 fallbacks.
const TEXT_TIMEOUT: Duration = Duration::from_secs(50);
/// Most fallback models the server accepts.
pub const MAX_FALLBACK_MODELS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

/// `context`: ids the server echoes back and records (meeting id, stable per-retry turn id).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// `routing`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routing {
    /// Tried in order after the primary model; at most 3.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_models: Vec<String>,
    /// 500–10000 ms per attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_timeout_ms: Option<u32>,
}

/// `POST /v1/text` body (strict on the server: only these fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<TextContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing: Option<Routing>,
}

impl TextRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            max_output_tokens: None,
            temperature: None,
            context: None,
            routing: None,
        }
    }
}

/// Token usage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub provider_cost_usd: Option<f64>,
}

/// `POST /v1/text` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResponse {
    #[serde(default)]
    pub text: String,
    /// The model that answered (a fallback when the primary failed).
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub upstream_model: Option<String>,
    /// `stop`, `length`, …
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub provider_latency_ms: Option<f64>,
    #[serde(default)]
    pub usage: TextUsage,
    #[serde(default)]
    pub context: Option<Value>,
    #[serde(default)]
    pub warnings: Vec<Value>,
}

impl VoiceClient {
    /// Generate text. `finishReason: "length"` is returned as [`Error::Truncated`] with the partial response.
    pub async fn generate(&self, request: &TextRequest) -> Result<TextResponse> {
        validate(request)?;
        let response = self
            .api()
            .post(self.url("v1/text")?)
            .timeout(TEXT_TIMEOUT)
            .json(request)
            .send()
            .await?;
        let response: TextResponse = decode(response).await?;
        if response.finish_reason.as_deref() == Some("length") {
            return Err(Error::Truncated(Box::new(response)));
        }
        if response.text.trim().is_empty() {
            return Err(Error::Decode(format!(
                "model {} returned no text",
                response.model
            )));
        }
        Ok(response)
    }
}

fn validate(request: &TextRequest) -> Result<()> {
    if request.model.trim().is_empty() {
        return Err(Error::Config("text model is empty".into()));
    }
    if request.messages.is_empty() || request.messages.iter().any(|m| m.content.is_empty()) {
        return Err(Error::Config("text messages must be non-empty".into()));
    }
    if let Some(routing) = &request.routing {
        if routing.fallback_models.len() > MAX_FALLBACK_MODELS {
            return Err(Error::Config(format!(
                "at most {MAX_FALLBACK_MODELS} fallback models"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_shape() {
        let mut r = TextRequest::new(
            "google-gemini-3.8-flash",
            vec![Message::system("Summarize."), Message::user("transcript")],
        );
        r.max_output_tokens = Some(1500);
        r.context = Some(TextContext {
            conversation_id: Some("m1".into()),
            turn_id: None,
        });
        r.routing = Some(Routing {
            fallback_models: vec!["deepseek-deepseek-v4-flash".into()],
            attempt_timeout_ms: None,
        });
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "model": "google-gemini-3.8-flash",
                "messages": [{"role":"system","content":"Summarize."},{"role":"user","content":"transcript"}],
                "maxOutputTokens": 1500,
                "context": {"conversationId": "m1"},
                "routing": {"fallbackModels": ["deepseek-deepseek-v4-flash"]}
            })
        );
        assert!(validate(&r).is_ok());
        r.routing.as_mut().unwrap().fallback_models = vec!["a".into(); 4];
        assert!(matches!(validate(&r), Err(Error::Config(_))));
        assert!(matches!(
            validate(&TextRequest::new("m", vec![])),
            Err(Error::Config(_))
        ));
        assert!(matches!(
            validate(&TextRequest::new("m", vec![Message::user("")])),
            Err(Error::Config(_))
        ));
    }

    #[test]
    fn parses_live_response() {
        let body = r#"{"cache":{},"context":{"requestId":"request_c7","turnId":"turn_93"},"finishReason":"stop",
            "model":"google-gemini-3.8-flash","provider":"google","providerLatencyMs":4387,"upstreamModel":"gemini-3.8-flash",
            "usage":{"inputTokens":12464,"outputTokens":213,"textTokens":213,"totalTokens":12677},
            "warnings":[{"feature":"reasoning","type":"unsupported"}],"outputType":"text","text":"- Notes"}"#;
        let r: TextResponse = serde_json::from_str(body).unwrap();
        assert_eq!(r.text, "- Notes");
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
        assert_eq!(r.usage.total_tokens, Some(12677));
        assert_eq!(r.provider_latency_ms, Some(4387.0));
        assert_eq!(r.warnings.len(), 1);
    }
}
