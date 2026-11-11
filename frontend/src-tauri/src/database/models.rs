use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Meeting status values (`meetings.status`).
pub mod meeting_status {
    pub const RECORDING: &str = "recording";
    pub const STOPPED: &str = "stopped";
    pub const INTERRUPTED: &str = "interrupted";
    pub const IMPORTED: &str = "imported";
}

/// Transcript sources (`transcripts.source`).
pub mod transcript_source {
    pub const LIVE: &str = "live";
    pub const FINAL: &str = "final";
}

/// One row of `meetings`.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, PartialEq)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub folder_path: Option<String>,
    pub status: String,
    pub duration_seconds: Option<f64>,
    pub live_model: Option<String>,
    pub live_mode: Option<String>,
    /// JSON array of realtime session ids.
    pub realtime_session_ids: Option<String>,
    pub final_status: Option<String>,
    pub final_model: Option<String>,
    pub final_job_id: Option<String>,
    pub final_error: Option<String>,
    pub final_updated_at: Option<String>,
}

/// One row of `transcripts`.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, PartialEq)]
pub struct TranscriptRow {
    pub id: String,
    pub meeting_id: String,
    pub source: String,
    pub speaker: Option<String>,
    pub transcript: String,
    pub timestamp: Option<String>,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub duration: Option<f64>,
    pub sequence: Option<i64>,
}

/// A transcript line to insert (the repository assigns the id).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewTranscript {
    pub speaker: Option<String>,
    pub text: String,
    pub timestamp: Option<String>,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub duration: Option<f64>,
    pub sequence: Option<i64>,
}

/// One row of `summary_processes`.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SummaryProcess {
    pub meeting_id: String,
    pub status: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
    /// JSON
    pub result: Option<String>,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub processing_time: Option<f64>,
    /// JSON
    pub metadata: Option<String>,
    /// Previous result kept while a regeneration runs.
    pub result_backup: Option<String>,
    pub result_backup_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}
