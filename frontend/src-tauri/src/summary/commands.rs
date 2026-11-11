use crate::database::repositories::{meeting::MeetingsRepository, summary::SummaryProcessesRepository};
use crate::state::AppState;
use crate::summary::processor::{clean_llm_markdown_detailed, contains_reasoning_marker};
use crate::summary::service::{SummaryJob, SummaryService};
use crate::summary::templates::DEFAULT_TEMPLATE_ID;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use log::{error as log_error, info as log_info, warn as log_warn};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
use tauri::AppHandle;

static SUMMARY_START_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));
static LAST_SUMMARY_START: LazyLock<Mutex<Option<DateTime<Utc>>>> =
    LazyLock::new(|| Mutex::new(None));
fn next_summary_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let mut last = LAST_SUMMARY_START
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let next = match *last {
        Some(previous) if now <= previous => previous + Duration::nanoseconds(1),
        _ => now,
    };
    *last = Some(next);
    next
}
#[derive(Debug, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub status: String,
    #[serde(rename = "meetingName")]
    pub meeting_name: Option<String>,
    pub meeting_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessTranscriptResponse {
    pub process_id: String,
}

/// Saves a meeting summary.
///
/// Expected format: { "markdown": "...", "summary_json": [...BlockNote blocks...] }
#[tauri::command]
pub async fn api_save_meeting_summary(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary: serde_json::Value,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_meeting_summary called for meeting_id: {}",
        meeting_id
    );
    if !summary_is_renderable(&summary) {
        return Err("Summary contains no visible content or model reasoning markers and was not saved.".into());
    }
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::update_meeting_summary(pool, &meeting_id, &summary).await {
        Ok(true) => {
            log_info!("Summary saved successfully for meeting_id: {}", meeting_id);
            Ok(serde_json::json!({
                "message": "Meeting summary saved successfully"
            }))
        }
        Ok(false) => {
            log_warn!(
                "Meeting not found or invalid JSON for meeting_id: {}",
                meeting_id
            );
            Err("Meeting not found or can't convert the json".into())
        }
        Err(e) => {
            log_error!("Failed to save meeting summary for {}: {}", meeting_id, e);
            Err(e.to_string())
        }
    }
}

fn blocknote_has_visible_text(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(blocknote_has_visible_text),
        serde_json::Value::Object(object) => {
            object
                .get("text")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
                || object
                    .get("content")
                    .is_some_and(blocknote_has_visible_text)
                || object
                    .get("children")
                    .is_some_and(blocknote_has_visible_text)
        }
        _ => false,
    }
}



fn summary_contains_reasoning_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(summary_contains_reasoning_marker),
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| match key.as_str() {
            "markdown" | "text" | "content" => value
                .as_str()
                .is_some_and(contains_reasoning_marker)
                || value.is_array() && summary_contains_reasoning_marker(value),
            "summary_json" | "children" => summary_contains_reasoning_marker(value),
            _ => false,
        }),
        _ => false,
    }
}

fn summary_is_renderable(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if summary_contains_reasoning_marker(value) {
        return false;
    }
    if let Some(markdown) = object.get("markdown") {
        return markdown
            .as_str()
            .is_some_and(|markdown| !clean_llm_markdown_detailed(markdown).markdown.is_empty());
    }
    object.get("summary_json").is_some_and(blocknote_has_visible_text)
}

fn sanitize_markdown_value(value: Option<&mut serde_json::Value>) -> bool {
    let Some(serde_json::Value::String(markdown)) = value else {
        return false;
    };
    let cleaned = clean_llm_markdown_detailed(markdown);
    let reasoning_stripped = cleaned.reasoning_stripped;
    *markdown = cleaned.markdown;
    reasoning_stripped
}

/// Strip model reasoning from a stored summary's `markdown` and flag it when any was removed.
fn sanitize_summary(mut result: serde_json::Value) -> serde_json::Value {
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    if sanitize_markdown_value(object.get_mut("markdown")) {
        object.insert("reasoning_stripped".to_string(), serde_json::Value::Bool(true));
    }
    result
}

/// Gets summary status and data.
///
/// Returns summary status (pending/processing/completed/failed) and parsed result data
#[tauri::command]
pub async fn api_get_summary(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<SummaryResponse, String> {
    log_info!(
        "api_get_summary called for meeting_id: {}",
        meeting_id
    );
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::get_summary_data(pool, &meeting_id).await {
        Ok(Some(process)) => {
            let mut status = process.status.unwrap_or_default().to_lowercase();
            let mut error = process.error;

            // Parse result data if it exists (regardless of status) so restored
            // summaries remain available after cancellation or failure.
            let mut data = if let Some(result_str) = process.result {
                match serde_json::from_str::<serde_json::Value>(&result_str) {
                    Ok(parsed) => Some(sanitize_summary(parsed)),
                    Err(e) => {
                        log_error!("Failed to parse summary result JSON: {}", e);
                        None
                    }
                }
            } else {
                None
            };
            if status == "completed" && !data.as_ref().is_some_and(summary_is_renderable) {
                status = "error".to_string();
                error = Some(
                    "Stored summary data is missing or invalid. Generate the summary again."
                        .to_string(),
                );
                data = None;
            }

            // Fetch meeting title from database
            let meeting_name = meeting_title(pool, &meeting_id).await;

            let response = SummaryResponse {
                status: status.clone(),
                meeting_name,
                meeting_id: meeting_id.clone(),
                start: process
                    .start_time
                    .map(|time| time.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                end: process.end_time.map(|time| time.to_rfc3339()),
                data,
                error,
            };

            log_info!(
                "Summary status for {}: {}, has_data: {}, meeting_name: {:?}",
                meeting_id,
                status,
                response.data.is_some(),
                response.meeting_name
            );
            Ok(response)
        }
        Ok(None) => {
            log_info!("No summary process found for meeting_id: {}", meeting_id);

            // Still fetch meeting title for idle state
            let meeting_name = meeting_title(pool, &meeting_id).await;

            Ok(SummaryResponse {
                status: "idle".to_string(),
                meeting_name,
                meeting_id,
                start: None,
                end: None,
                data: None,
                error: None,
            })
        }
        Err(e) => {
            log_error!("Error retrieving summary for {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve summary: {}", e))
        }
    }
}

async fn meeting_title(pool: &sqlx::SqlitePool, meeting_id: &str) -> Option<String> {
    match MeetingsRepository::get(pool, meeting_id).await {
        Ok(meeting) => meeting.map(|m| m.title),
        Err(e) => {
            log_error!("Failed to fetch meeting title for {}: {}", meeting_id, e);
            None
        }
    }
}

/// Start generating the summary of a meeting from its best transcript (read from the
/// database). Returns immediately with the `process_id` of this generation. Used by the
/// command and by the final transcript when `auto_summary` is on.
pub async fn start_summary(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    template_id: Option<String>,
    custom_prompt: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    if MeetingsRepository::get(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .is_none()
    {
        return Err(format!("Meeting not found: {}", meeting_id));
    }
    let template_id = template_id
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string());
    let model = model.filter(|m| !m.trim().is_empty());
    log_info!(
        "Starting summary for {} (template {}, model {:?})",
        meeting_id,
        template_id,
        model
    );

    let _start_guard = SUMMARY_START_LOCK.lock().await;
    let started_at = next_summary_start(Utc::now());
    SummaryProcessesRepository::create_or_reset_process(pool, meeting_id, started_at)
        .await
        .map_err(|e| format!("Failed to initialize process: {}", e))?;

    let cancellation_token = SummaryService::register_cancellation_token(meeting_id, started_at);
    let job = SummaryJob {
        meeting_id: meeting_id.to_string(),
        started_at,
        template_id,
        custom_prompt: custom_prompt.unwrap_or_default(),
        model,
    };
    tauri::async_runtime::spawn(SummaryService::process_transcript_background(
        app.clone(),
        pool.clone(),
        job,
        cancellation_token,
    ));
    Ok(started_at.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

#[tauri::command]
pub async fn api_process_transcript(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    template_id: Option<String>,
    custom_prompt: Option<String>,
    model: Option<String>,
) -> Result<ProcessTranscriptResponse, String> {
    let pool = state.db_manager.pool().clone();
    let process_id = start_summary(&app, &pool, &meeting_id, template_id, custom_prompt, model).await?;
    Ok(ProcessTranscriptResponse { process_id })
}

/// Cancels an ongoing summary generation process
///
/// This command triggers the cancellation token for the specified meeting,
/// stopping the summary generation gracefully.
#[tauri::command]
pub async fn api_cancel_summary(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    process_id: String,
) -> Result<serde_json::Value, String> {
    let started_at = DateTime::parse_from_rfc3339(&process_id)
        .map_err(|_| "Invalid summary process ID".to_string())?
        .with_timezone(&Utc);
    log_info!("api_cancel_summary called for meeting_id: {}", meeting_id);

    let cancellation_requested = SummaryService::cancel_summary(&meeting_id, started_at);
    let cancelled = if cancellation_requested {
        let pool = state.db_manager.pool();
        match SummaryProcessesRepository::update_process_cancelled(pool, &meeting_id, started_at)
            .await
        {
            Ok(true) => {
                log_info!("Successfully cancelled summary generation for meeting_id: {}", meeting_id);
                true
            }
            Ok(false) => {
                log_info!("Summary generation was already terminal for meeting_id: {}", meeting_id);
                false
            }
            Err(error) => {
                log_error!("Failed to update cancellation status for {}: {}", meeting_id, error);
                return Err(format!("Failed to update cancellation status: {}", error));
            }
        }
    } else {
        false
    };

    Ok(serde_json::json!({
        "cancelled": cancelled,
        "message": if cancelled {
            "Summary generation cancelled successfully"
        } else if cancellation_requested {
            "Summary generation was already terminal"
        } else {
            "No active summary generation to cancel"
        },
        "meeting_id": meeting_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;



    #[test]
    fn summary_start_tokens_are_monotonic_with_identical_clock_inputs() {
        let first = next_summary_start(Utc::now());
        assert_eq!(
            next_summary_start(first),
            first + Duration::nanoseconds(1)
        );
    }

    #[test]
    fn summary_validation_rejects_blank_and_reasoning_content() {
        assert!(!summary_is_renderable(&json!({"markdown": "   "})));
        assert!(!summary_is_renderable(&json!({"markdown": "```\r\n```"})));
        assert!(!summary_is_renderable(&json!({
            "summary_json": [{"content": [{"type": "text", "text": "<think>private</think>"}]}]
        })));
        assert!(summary_is_renderable(&json!({
            "summary_json": [{"content": [{"type": "text", "text": "Visible"}]}]
        })));
    }

    #[test]
    fn response_redaction_strips_closed_markdown_envelopes() {
        let redacted = sanitize_summary(json!({
            "markdown": "Visible\n<think>private</think>\nTail"
        }));
        assert_eq!(redacted["markdown"], "Visible\n\nTail");
        assert_eq!(redacted["reasoning_stripped"], true);
    }
}
