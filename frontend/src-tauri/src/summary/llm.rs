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
    for candidate in req
        .fallback_models
        .as_ref()
        .unwrap_or(&settings.summary_fallback_models)
    {
        let candidate = candidate.trim();
        if !candidate.is_empty() && candidate != model && !fallbacks.iter().any(|f| f == candidate)
        {
            fallbacks.push(candidate.to_string());
        }
    }
    fallbacks.truncate(MAX_FALLBACK_MODELS);

    let mut request = TextRequest::new(
        model,
        vec![Message::system(&req.system), Message::user(&req.user)],
    );
    request.max_output_tokens = Some(req.max_output_tokens);
    request.context = Some(TextContext {
        conversation_id: Some(req.conversation_id.clone()),
        turn_id: Some(req.turn_id.clone()),
    });
    request.routing = (!fallbacks.is_empty()).then_some(Routing {
        fallback_models: fallbacks,
        attempt_timeout_ms: None,
    });
    request
}

// Desktop summaries and speaker review share this queue. The guard sets the
// quiet period on drop, including when a waiting generation is cancelled mid-flight.
type Deadline = Option<tokio::time::Instant>;
static TEXT_QUEUE: tokio::sync::Mutex<Deadline> = tokio::sync::Mutex::const_new(None);
struct QuietPeriod<'a> {
    deadline: tokio::sync::MutexGuard<'a, Deadline>,
    delay: std::time::Duration,
}
impl Drop for QuietPeriod<'_> {
    fn drop(&mut self) {
        *self.deadline = tokio::time::Instant::now().checked_add(self.delay);
    }
}

/// Generate one completion with an explicit client (no app state).
pub async fn generate_with(
    pool: &sqlx::SqlitePool,
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
    let deadline = TEXT_QUEUE.lock().await;
    if let Some(next) = *deadline {
        tokio::time::sleep_until(next).await;
    }
    let eligible_ms: i64 =
        sqlx::query_scalar("SELECT eligible_ms FROM text_request_schedule WHERE id=1")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    let remaining = eligible_ms.saturating_sub(chrono::Utc::now().timestamp_millis());
    if remaining > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(remaining as u64)).await;
    }
    let mut quiet = QuietPeriod {
        deadline,
        delay: std::time::Duration::from_secs(15),
    };
    let response = client.generate(&request).await;
    if let Err(peacockery_voice::Error::TextBackoff { delay, .. }) = &response {
        quiet.delay = quiet.delay.max(*delay);
    }
    let eligible = chrono::Utc::now()
        .timestamp_millis()
        .saturating_add(i64::try_from(quiet.delay.as_millis()).unwrap_or(i64::MAX));
    sqlx::query("UPDATE text_request_schedule SET eligible_ms=? WHERE id=1")
        .bind(eligible)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let response = response.map_err(|e| describe_error(&e))?;
    Ok(SummaryLlmResponse {
        text: response.text,
        model: response.model,
        provider: response.provider.unwrap_or_default(),
        usage: serde_json::to_value(&response.usage).unwrap_or(serde_json::Value::Null),
    })
}

/// Generate one completion for a summary.
pub async fn generate(
    app: &tauri::AppHandle,
    req: SummaryLlmRequest,
) -> Result<SummaryLlmResponse, String> {
    let voice = app.state::<VoiceService>();
    let client = voice.client()?;
    let state = app.state::<crate::state::AppState>();
    generate_with(state.db_manager.pool(), &client, &voice.settings(), &req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_delay_survives_a_fresh_queue() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::time::{timeout, Duration};
        let pool = crate::database::test_pool().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = VoiceClient::new(peacockery_voice::ClientConfig::new(
            format!("http://{}", listener.local_addr().unwrap()),
            "fixture",
        ))
        .unwrap();
        let serve = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut chunk = [0; 4096];
            assert!(conn.read(&mut chunk).await.unwrap() > 0);
            conn.write_all(b"HTTP/1.1 429 Too Many Requests\r\nretry-after: 60\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
            listener
        });
        let settings = VoiceSettings::default();
        let req = request(None, None);
        assert!(generate_with(&pool, &client, &settings, &req)
            .await
            .is_err());
        let listener = serve.await.unwrap();
        let eligible: i64 = sqlx::query_scalar("SELECT eligible_ms FROM text_request_schedule")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(eligible - chrono::Utc::now().timestamp_millis() > 59000);
        *TEXT_QUEUE.lock().await = None;
        assert!(timeout(
            Duration::from_millis(100),
            generate_with(&pool, &client, &settings, &req)
        )
        .await
        .is_err());
        assert!(timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err());
    }

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
            &request(
                Some("anthropic-claude-sonnet-5-5"),
                Some(vec!["anthropic-claude-sonnet-5-5", "a", "a", "b", "c", "d"]),
            ),
        );
        assert_eq!(r.model, "anthropic-claude-sonnet-5-5");
        assert_eq!(r.routing.unwrap().fallback_models, vec!["a", "b", "c"]);
        let none = text_request(&settings, &request(Some(" "), Some(vec![])));
        assert_eq!(none.model, "google-gemini-3.8-flash");
        assert!(none.routing.is_none());
    }
}
