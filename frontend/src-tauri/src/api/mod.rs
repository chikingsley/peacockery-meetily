//! Meeting and transcript commands (ARCHITECTURE.md §8 "Meetings / transcripts").

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::database::models::{Meeting, TranscriptRow};
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::transcript::{SourceChoice, TranscriptSearchResult, TranscriptsRepository};
use crate::state::AppState;

/// Sidebar row.
#[derive(Debug, Serialize)]
pub struct MeetingListItem {
    pub id: String,
    pub title: String,
    pub created_at: Option<String>,
    pub final_status: Option<String>,
}

/// Every meeting column plus the audio file and which transcript sources exist.
#[derive(Debug, Serialize)]
pub struct MeetingMetadata {
    #[serde(flatten)]
    pub meeting: Meeting,
    /// The meeting's `audio.<ext>` (recordings are `audio.mp4`, imports keep their
    /// extension); `None` when the folder has no audio file.
    pub audio_path: Option<String>,
    pub has_live: bool,
    pub has_final: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Transcript {
    pub id: String,
    pub text: String,
    pub timestamp: Option<String>,
    pub speaker: Option<String>,
    pub source: String,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub duration: Option<f64>,
}

impl From<TranscriptRow> for Transcript {
    fn from(row: TranscriptRow) -> Self {
        Self {
            id: row.id,
            text: row.transcript,
            timestamp: row.timestamp,
            speaker: row.speaker,
            source: row.source,
            audio_start_time: row.audio_start_time,
            audio_end_time: row.audio_end_time,
            duration: row.duration,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PaginatedTranscriptsResponse {
    pub transcripts: Vec<Transcript>,
    pub total_count: i64,
    pub has_more: bool,
    /// The source the rows came from (`live` or `final`).
    pub source: String,
}

fn db_error(context: &str, error: sqlx::Error) -> String {
    log::error!("{}: {}", context, error);
    format!("{}: {}", context, error)
}

async fn require_meeting(pool: &sqlx::SqlitePool, meeting_id: &str) -> Result<Meeting, String> {
    MeetingsRepository::get(pool, meeting_id)
        .await
        .map_err(|e| db_error("Failed to load meeting", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))
}

#[tauri::command]
pub async fn api_get_meetings(state: State<'_, AppState>) -> Result<Vec<MeetingListItem>, String> {
    let meetings = MeetingsRepository::list(state.db_manager.pool())
        .await
        .map_err(|e| db_error("Failed to list meetings", e))?;
    Ok(meetings
        .into_iter()
        .map(|m| MeetingListItem { id: m.id, title: m.title, created_at: m.created_at, final_status: m.final_status })
        .collect())
}

#[tauri::command]
pub async fn api_search_transcripts(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<TranscriptSearchResult>, String> {
    TranscriptsRepository::search(state.db_manager.pool(), &query)
        .await
        .map_err(|e| db_error("Failed to search transcripts", e))
}

#[tauri::command]
pub async fn api_get_meeting_metadata(
    app: AppHandle,
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingMetadata, String> {
    let pool = state.db_manager.pool();
    let meeting = require_meeting(pool, &meeting_id).await?;
    let (live, final_count) = TranscriptsRepository::source_counts(pool, &meeting_id)
        .await
        .map_err(|e| db_error("Failed to count transcripts", e))?;
    let audio = meeting
        .folder_path
        .as_deref()
        .and_then(|folder| crate::audio::constants::find_meeting_audio(std::path::Path::new(folder)));
    if let Some(path) = &audio {
        // The meeting may live outside the current recordings folder (older save folder).
        if let Err(e) = app.asset_protocol_scope().allow_file(path) {
            log::warn!("Failed to allow {} for audio playback: {}", path.display(), e);
        }
    }
    Ok(MeetingMetadata {
        meeting,
        audio_path: audio.map(|path| path.to_string_lossy().to_string()),
        has_live: live > 0,
        has_final: final_count > 0,
    })
}

/// One page of a meeting's transcript. `source`: `best` (default; final when present,
/// else live), `live` or `final`.
#[tauri::command]
pub async fn api_get_meeting_transcripts(
    state: State<'_, AppState>,
    meeting_id: String,
    limit: i64,
    offset: i64,
    source: Option<String>,
) -> Result<PaginatedTranscriptsResponse, String> {
    let pool = state.db_manager.pool();
    let choice = SourceChoice::parse(source.as_deref())?;
    let source = TranscriptsRepository::resolve_source(pool, &meeting_id, choice)
        .await
        .map_err(|e| db_error("Failed to read transcripts", e))?;
    let (rows, total_count) = TranscriptsRepository::page(pool, &meeting_id, source, limit, offset)
        .await
        .map_err(|e| db_error("Failed to read transcripts", e))?;
    let has_more = offset + (rows.len() as i64) < total_count;
    Ok(PaginatedTranscriptsResponse {
        transcripts: rows.into_iter().map(Transcript::from).collect(),
        total_count,
        has_more,
        source: source.to_string(),
    })
}

#[tauri::command]
pub async fn api_save_meeting_title(
    state: State<'_, AppState>,
    meeting_id: String,
    title: String,
) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Title cannot be empty".to_string());
    }
    let updated = MeetingsRepository::update_title(state.db_manager.pool(), &meeting_id, title)
        .await
        .map_err(|e| db_error("Failed to save title", e))?;
    if updated {
        Ok(())
    } else {
        Err(format!("Meeting not found: {}", meeting_id))
    }
}

/// Delete the meeting row (with its transcripts and summary). The recording folder on
/// disk is kept.
#[tauri::command]
pub async fn api_delete_meeting(state: State<'_, AppState>, meeting_id: String) -> Result<(), String> {
    let deleted = MeetingsRepository::delete(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| db_error("Failed to delete meeting", e))?;
    if deleted {
        Ok(())
    } else {
        Err(format!("Meeting not found: {}", meeting_id))
    }
}

#[tauri::command]
pub async fn open_meeting_folder(state: State<'_, AppState>, meeting_id: String) -> Result<(), String> {
    let meeting = require_meeting(state.db_manager.pool(), &meeting_id).await?;
    let folder = meeting
        .folder_path
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "This meeting has no recording folder".to_string())?;
    if !folder.is_dir() {
        return Err(format!("Recording folder not found: {}", folder.display()));
    }
    crate::utils::open_folder(&folder)
}

/// Rename speaker `from` to `to` on every transcript row (live and final) of a meeting.
/// Returns the number of rows changed.
#[tauri::command]
pub async fn rename_speaker(
    state: State<'_, AppState>,
    meeting_id: String,
    from: String,
    to: String,
) -> Result<u64, String> {
    let to = to.trim();
    if to.is_empty() {
        return Err("Speaker name cannot be empty".to_string());
    }
    TranscriptsRepository::rename_speaker(state.db_manager.pool(), &meeting_id, &from, to)
        .await
        .map_err(|e| db_error("Failed to rename speaker", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_flattens_meeting_columns() {
        let meeting = Meeting {
            id: "m1".into(),
            title: "Call".into(),
            created_at: Some("2026-11-09T10:00:00+00:00".into()),
            updated_at: None,
            folder_path: Some("C:/Music/Call".into()),
            status: "stopped".into(),
            duration_seconds: Some(12.0),
            live_model: None,
            live_mode: None,
            realtime_session_ids: None,
            final_status: Some("done".into()),
            final_model: None,
            final_job_id: None,
            final_error: None,
            final_updated_at: None,
        };
        let value = serde_json::to_value(MeetingMetadata {
            meeting,
            audio_path: Some("C:/Music/Call/audio.m4a".into()),
            has_live: true,
            has_final: false,
        })
        .unwrap();
        assert_eq!(value["audio_path"], "C:/Music/Call/audio.m4a");
        assert_eq!(value["id"], "m1");
        assert_eq!(value["status"], "stopped");
        assert_eq!(value["final_status"], "done");
        assert_eq!(value["has_live"], true);
        assert_eq!(value["has_final"], false);
    }
}
