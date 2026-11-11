//! Background summary generation: one LLM pass over the meeting's best transcript.

use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;
use crate::database::repositories::transcript::{SourceChoice, TranscriptsRepository};
use crate::summary::llm::{self, SummaryLlmRequest};
use crate::summary::processor::{
    build_user_prompt, clean_llm_markdown_detailed, extract_meeting_name_from_markdown, format_transcript,
    require_visible_markdown, strip_title_if_present, SUMMARY_SYSTEM_PROMPT,
};
use crate::summary::templates;
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Output budget for one summary.
const SUMMARY_MAX_OUTPUT_TOKENS: u32 = 4096;

#[derive(Clone)]
struct RegisteredCancellation {
    started_at: DateTime<Utc>,
    token: CancellationToken,
}

static CANCELLATION_REGISTRY: LazyLock<Mutex<HashMap<String, RegisteredCancellation>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// What to summarize and how.
#[derive(Debug, Clone)]
pub struct SummaryJob {
    pub meeting_id: String,
    pub started_at: DateTime<Utc>,
    pub template_id: String,
    pub custom_prompt: String,
    pub model: Option<String>,
}

pub struct SummaryService;

impl SummaryService {
    /// Register the token of a new generation; cancels the previous one of that meeting.
    pub(crate) fn register_cancellation_token(meeting_id: &str, started_at: DateTime<Utc>) -> CancellationToken {
        let token = CancellationToken::new();
        let mut registry = CANCELLATION_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = registry.insert(
            meeting_id.to_string(),
            RegisteredCancellation { started_at, token: token.clone() },
        ) {
            previous.token.cancel();
        }
        token
    }

    /// Cancel only the generation identified by `started_at`.
    pub fn cancel_summary(meeting_id: &str, started_at: DateTime<Utc>) -> bool {
        let registry = CANCELLATION_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        match registry.get(meeting_id).filter(|entry| entry.started_at == started_at) {
            Some(entry) => {
                entry.token.cancel();
                true
            }
            None => false,
        }
    }

    fn cleanup_cancellation_token(meeting_id: &str, started_at: DateTime<Utc>) {
        let mut registry = CANCELLATION_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        if registry
            .get(meeting_id)
            .is_some_and(|entry| entry.started_at == started_at)
        {
            registry.remove(meeting_id);
        }
    }

    /// Generate and store the summary; every terminal write is guarded by `started_at`
    /// so a stale generation never overwrites a newer one.
    pub async fn process_transcript_background(
        app: tauri::AppHandle,
        pool: SqlitePool,
        job: SummaryJob,
        cancellation_token: CancellationToken,
    ) {
        let started = Instant::now();
        let outcome = tokio::select! {
            outcome = Self::generate(&app, &pool, &job) => outcome,
            _ = cancellation_token.cancelled() => Err("Summary generation was cancelled".to_string()),
        };
        let duration = started.elapsed().as_secs_f64();

        match outcome {
            Ok((result, metadata, title)) => {
                match SummaryProcessesRepository::update_process_completed(
                    &pool,
                    &job.meeting_id,
                    job.started_at,
                    result,
                    Some(metadata),
                    duration,
                )
                .await
                {
                    Ok(true) => {
                        if let Some(title) = title {
                            if let Err(e) = MeetingsRepository::update_title(&pool, &job.meeting_id, &title).await {
                                log::error!("Failed to rename meeting {}: {}", job.meeting_id, e);
                            }
                        }
                        log::info!("Summary saved for {} in {:.1}s", job.meeting_id, duration);
                    }
                    Ok(false) => log::warn!("Skipped stale summary completion for {}", job.meeting_id),
                    Err(e) => log::error!("Failed to save summary for {}: {}", job.meeting_id, e),
                }
            }
            Err(_) if cancellation_token.is_cancelled() => {
                match SummaryProcessesRepository::update_process_cancelled(&pool, &job.meeting_id, job.started_at).await {
                    Ok(_) => log::info!("Summary cancelled for {}", job.meeting_id),
                    Err(e) => log::error!("Failed to mark summary cancelled for {}: {}", job.meeting_id, e),
                }
            }
            Err(error) => {
                log::error!("Summary failed for {}: {}", job.meeting_id, error);
                if let Err(e) =
                    SummaryProcessesRepository::update_process_failed(&pool, &job.meeting_id, job.started_at, &error).await
                {
                    log::error!("Failed to mark summary failed for {}: {}", job.meeting_id, e);
                }
            }
        }
        Self::cleanup_cancellation_token(&job.meeting_id, job.started_at);
    }

    /// Returns `(result json, metadata json, title from the summary)`.
    async fn generate(
        app: &tauri::AppHandle,
        pool: &SqlitePool,
        job: &SummaryJob,
    ) -> Result<(serde_json::Value, serde_json::Value, Option<String>), String> {
        let template = templates::get_template(&job.template_id)?;
        let source = TranscriptsRepository::resolve_source(pool, &job.meeting_id, SourceChoice::Best)
            .await
            .map_err(|e| format!("Failed to read transcript: {}", e))?;
        let rows = TranscriptsRepository::all(pool, &job.meeting_id, source)
            .await
            .map_err(|e| format!("Failed to read transcript: {}", e))?;
        let transcript = format_transcript(&rows);
        if transcript.is_empty() {
            return Err("This meeting has no transcript to summarize yet".to_string());
        }

        let request = SummaryLlmRequest {
            model: job.model.clone(),
            fallback_models: None,
            system: SUMMARY_SYSTEM_PROMPT.to_string(),
            user: build_user_prompt(&transcript, &template, &job.custom_prompt),
            max_output_tokens: SUMMARY_MAX_OUTPUT_TOKENS,
            conversation_id: format!("meetily:{}", job.meeting_id),
            turn_id: format!(
                "summary:{}:{}",
                job.meeting_id,
                job.started_at.to_rfc3339_opts(SecondsFormat::Nanos, true)
            ),
        };
        let response = llm::generate(app, request).await?;

        let cleaned = clean_llm_markdown_detailed(&response.text);
        require_visible_markdown("Summary", &cleaned)?;
        let title = extract_meeting_name_from_markdown(&cleaned.markdown).filter(|t| !t.is_empty());
        let markdown = strip_title_if_present(&cleaned.markdown);
        if markdown.trim().is_empty() {
            return Err("Summary contains no visible content after title removal".to_string());
        }

        let result = serde_json::json!({
            "markdown": markdown,
            "reasoning_stripped": cleaned.reasoning_stripped,
        });
        let metadata = serde_json::json!({
            "model": response.model,
            "provider": response.provider,
            "usage": response.usage,
            "template_id": job.template_id,
            "transcript_source": source,
        });
        Ok((result, metadata, title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_cleanup_keeps_the_replacement_cancellation_token() {
        let meeting_id = format!("summary-test-{}", uuid::Uuid::new_v4());
        let first_started_at = Utc::now();
        let second_started_at = first_started_at + chrono::Duration::nanoseconds(1);
        let first = SummaryService::register_cancellation_token(&meeting_id, first_started_at);
        let second = SummaryService::register_cancellation_token(&meeting_id, second_started_at);
        assert!(first.is_cancelled(), "registering a new generation cancels the old one");

        SummaryService::cleanup_cancellation_token(&meeting_id, first_started_at);
        assert!(!SummaryService::cancel_summary(&meeting_id, first_started_at));
        assert!(SummaryService::cancel_summary(&meeting_id, second_started_at));
        assert!(second.is_cancelled());
        SummaryService::cleanup_cancellation_token(&meeting_id, second_started_at);
    }
}
