//! Final transcript (ARCHITECTURE.md §6): the meeting's audio file → Peacockery Voice batch job →
//! turns → speaker names → `source='final'` rows, `final-transcript.json`, `final-artifact.json`.
//!
//! One job runs per meeting at a time; starting again cancels the running one and replaces it.
//! [`run_final_transcript`] is the core (no Tauri); the rest queues it and reports state through
//! the `meetings.final_*` columns and the `final-transcript-status` event.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use chrono::{DateTime, Local};
use peacockery_voice::{
    name_speakers, turns_with_source, Artifact, BatchProgress, CancellationToken, EnergyTrack, Error, ModelFamily,
    ProgressFn, TranscribeOptions, Turn, TurnSource, VoiceClient,
};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::constants::find_meeting_audio;
use crate::database::models::{meeting_status, transcript_source, Meeting, NewTranscript};
use crate::database::repositories::meeting::{FinalState, MeetingsRepository};
use crate::database::repositories::transcript::TranscriptsRepository;
use crate::state::AppState;
use crate::voice::settings::is_final_model;
use crate::voice::{describe_error, VoiceService, VoiceSettings};

/// `meetings.final_status` / event `state` values.
pub mod final_state {
    pub const QUEUED: &str = "queued";
    pub const UPLOADING: &str = "uploading";
    pub const TRANSCRIBING: &str = "transcribing";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
}

pub const ENERGY_FILE: &str = "channel-energy.json";
pub const TRANSCRIPT_FILE: &str = "final-transcript.json";
pub const ARTIFACT_FILE: &str = "final-artifact.json";
const EVENT: &str = "final-transcript-status";

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

/// Everything the core needs about one run.
#[derive(Debug, Clone)]
pub struct FinalJob {
    pub meeting_id: String,
    pub folder: PathBuf,
    pub audio: PathBuf,
    pub options: TranscribeOptions,
    pub me_label: String,
    pub them_label: String,
    /// Wall-clock start of the recording, for the `HH:MM:SS` row timestamps (`None` for imports).
    pub recorded_at: Option<DateTime<Local>>,
}

impl FinalJob {
    /// Job options from the settings for `model`.
    pub fn new(meeting: &Meeting, folder: PathBuf, audio: PathBuf, model: &str, settings: &VoiceSettings) -> Self {
        let mut options = TranscribeOptions::new(model);
        // Cohere has no diarization; asking for it fails the job.
        options.diarize = settings.final_diarize && ModelFamily::of(model) != ModelFamily::Cohere;
        options.language = Some(settings.language.clone());
        options.keyterms = settings.keyterms.clone();
        let recorded_at = (meeting.status != meeting_status::IMPORTED)
            .then_some(meeting.created_at.as_deref())
            .flatten()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Local));
        Self {
            meeting_id: meeting.id.clone(),
            folder,
            audio,
            options,
            me_label: settings.me_label.clone(),
            them_label: settings.them_label.clone(),
            recorded_at,
        }
    }
}

/// Progress of a run, coarse enough to show and store.
#[derive(Debug, Clone, PartialEq)]
pub enum FinalProgress {
    /// Upload percentage (reported at 0 and every 10 %).
    Uploading { percent: u32 },
    /// The job exists and is queued or running at the provider.
    Transcribing { job_id: String },
    /// A poll carried provider retry text while the job waits.
    Waiting { job_id: String, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalOutcome {
    pub job_id: String,
    pub segments_count: usize,
    pub turn_source: TurnSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FinalError {
    Cancelled,
    Failed(String),
}

impl From<Error> for FinalError {
    fn from(error: Error) -> Self {
        match error {
            Error::Cancelled => FinalError::Cancelled,
            other => FinalError::Failed(describe_error(&other)),
        }
    }
}

/// `final-transcript.json`.
#[derive(Debug, Serialize)]
struct TranscriptFile<'a> {
    meeting_id: &'a str,
    model: &'a str,
    job_id: &'a str,
    turn_source: TurnSource,
    turns: &'a [Turn],
}

/// The energy track the recording wrote, if any and readable.
pub fn load_energy(folder: &Path) -> Option<EnergyTrack> {
    let path = folder.join(ENERGY_FILE);
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice::<EnergyTrack>(&bytes) {
        Ok(track) => Some(track),
        Err(e) => {
            log::warn!("Ignoring unreadable {}: {}", path.display(), e);
            None
        }
    }
}

/// Rows for named turns, in order.
pub fn rows_from_turns(turns: &[Turn], recorded_at: Option<DateTime<Local>>) -> Vec<NewTranscript> {
    turns
        .iter()
        .enumerate()
        .map(|(i, turn)| NewTranscript {
            speaker: turn.speaker.clone(),
            text: turn.text.clone(),
            timestamp: recorded_at.map(|start| {
                (start + chrono::Duration::milliseconds((turn.start * 1000.0) as i64))
                    .format("%H:%M:%S")
                    .to_string()
            }),
            audio_start_time: Some(turn.start),
            audio_end_time: Some(turn.end),
            duration: Some((turn.end - turn.start).max(0.0)),
            sequence: Some(i as i64),
        })
        .collect()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Turn a finished artifact into named rows and files and replace the meeting's final rows.
pub async fn store_final(
    pool: &SqlitePool,
    job: &FinalJob,
    job_id: &str,
    artifact: &Artifact,
) -> Result<FinalOutcome, FinalError> {
    let (turns, turn_source) = turns_with_source(artifact);
    let energy = load_energy(&job.folder);
    let named = name_speakers(&turns, energy.as_ref(), &job.me_label, &job.them_label);
    let rows = rows_from_turns(&named, job.recorded_at);

    let failed = |what: &str, e: &dyn std::fmt::Display| FinalError::Failed(format!("Could not save {}: {}", what, e));
    let artifact_json = serde_json::to_vec_pretty(&artifact.raw).map_err(|e| failed(ARTIFACT_FILE, &e))?;
    write_atomic(&job.folder.join(ARTIFACT_FILE), &artifact_json).map_err(|e| failed(ARTIFACT_FILE, &e))?;
    let transcript_json = serde_json::to_vec_pretty(&TranscriptFile {
        meeting_id: &job.meeting_id,
        model: &job.options.asr_model,
        job_id,
        turn_source,
        turns: &named,
    })
    .map_err(|e| failed(TRANSCRIPT_FILE, &e))?;
    write_atomic(&job.folder.join(TRANSCRIPT_FILE), &transcript_json).map_err(|e| failed(TRANSCRIPT_FILE, &e))?;

    TranscriptsRepository::replace_source(pool, &job.meeting_id, transcript_source::FINAL, &rows)
        .await
        .map_err(|e| failed("the final transcript", &e))?;
    Ok(FinalOutcome { job_id: job_id.to_string(), segments_count: rows.len(), turn_source })
}

/// Upload, transcribe, name speakers and store. `on_progress` is called from the running task.
pub async fn run_final_transcript(
    client: &VoiceClient,
    pool: &SqlitePool,
    job: &FinalJob,
    cancel: &CancellationToken,
    on_progress: impl Fn(FinalProgress) + Send + Sync + 'static,
) -> Result<FinalOutcome, FinalError> {
    let last_percent = AtomicU32::new(u32::MAX);
    let progress: ProgressFn = Arc::new(move |p| match p {
        BatchProgress::Uploading { sent, total } => {
            let percent = sent.saturating_mul(100).checked_div(total).map_or(100, |p| p as u32) / 10 * 10;
            if last_percent.swap(percent, Ordering::Relaxed) != percent {
                on_progress(FinalProgress::Uploading { percent });
            }
        }
        BatchProgress::JobCreated { job_id } => on_progress(FinalProgress::Transcribing { job_id }),
        BatchProgress::Job { job_id, error: Some(message), .. } => {
            on_progress(FinalProgress::Waiting { job_id, message })
        }
        BatchProgress::Uploaded { .. } | BatchProgress::Job { .. } => {}
    });
    let done = client.transcribe_file(&job.audio, &job.options, cancel, progress).await?;
    if cancel.is_cancelled() {
        return Err(FinalError::Cancelled);
    }
    store_final(pool, job, &done.job_id, &done.artifact).await
}

// ---------------------------------------------------------------------------
// Queue (one job per meeting)
// ---------------------------------------------------------------------------

struct Slot {
    generation: u64,
    token: CancellationToken,
    /// Held for the whole run, so a replacement starts only after the old run stops writing.
    lock: Arc<tokio::sync::Mutex<()>>,
}

static SLOTS: LazyLock<Mutex<HashMap<String, Slot>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Cancel the meeting's current run (if any) and register a new one.
fn register(meeting_id: &str) -> (u64, CancellationToken, Arc<tokio::sync::Mutex<()>>) {
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::SeqCst);
    let token = CancellationToken::new();
    let mut slots = SLOTS.lock().unwrap_or_else(|p| p.into_inner());
    let lock = match slots.remove(meeting_id) {
        Some(previous) => {
            previous.token.cancel();
            previous.lock
        }
        None => Arc::new(tokio::sync::Mutex::new(())),
    };
    slots.insert(meeting_id.to_string(), Slot { generation, token: token.clone(), lock: lock.clone() });
    (generation, token, lock)
}

fn unregister(meeting_id: &str, generation: u64) {
    let mut slots = SLOTS.lock().unwrap_or_else(|p| p.into_inner());
    if slots.get(meeting_id).is_some_and(|slot| slot.generation == generation) {
        slots.remove(meeting_id);
    }
}

/// `final-transcript-status` payload.
#[derive(Debug, Clone, Serialize)]
pub struct FinalStatusEvent {
    pub meeting_id: String,
    pub state: String,
    pub model: String,
    pub message: Option<String>,
    pub segments_count: Option<usize>,
}

/// `get_final_transcript_status` result.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FinalStatus {
    pub state: Option<String>,
    pub model: Option<String>,
    pub error: Option<String>,
}

/// Writes state to the database and the event, unless this run was replaced.
struct Reporter {
    app: AppHandle,
    pool: SqlitePool,
    meeting_id: String,
    model: String,
    token: CancellationToken,
    job_id: Mutex<Option<String>>,
}

impl Reporter {
    async fn report(&self, state: &str, message: Option<String>, segments_count: Option<usize>, store: bool) {
        if self.token.is_cancelled() {
            return;
        }
        if store {
            let job_id = self.job_id.lock().unwrap_or_else(|p| p.into_inner()).clone();
            let error = (state == final_state::FAILED).then_some(message.as_deref()).flatten();
            let row = FinalState { status: state, model: &self.model, job_id: job_id.as_deref(), error };
            if let Err(e) = MeetingsRepository::set_final_state(&self.pool, &self.meeting_id, &row).await {
                log::error!("Failed to store final transcript state of {}: {}", self.meeting_id, e);
            }
        }
        let _ = self.app.emit(
            EVENT,
            FinalStatusEvent {
                meeting_id: self.meeting_id.clone(),
                state: state.to_string(),
                model: self.model.clone(),
                message,
                segments_count,
            },
        );
    }

    async fn progress(&self, progress: FinalProgress, uploading_stored: &mut bool) {
        match progress {
            FinalProgress::Uploading { percent } => {
                let store = !*uploading_stored;
                *uploading_stored = true;
                self.report(final_state::UPLOADING, Some(format!("Uploading audio ({}%)", percent)), None, store)
                    .await;
            }
            FinalProgress::Transcribing { job_id } => {
                *self.job_id.lock().unwrap_or_else(|p| p.into_inner()) = Some(job_id);
                self.report(final_state::TRANSCRIBING, None, None, true).await;
            }
            FinalProgress::Waiting { message, .. } => {
                self.report(final_state::TRANSCRIBING, Some(message), None, false).await;
            }
        }
    }
}

fn pool_of(app: &AppHandle) -> SqlitePool {
    app.state::<AppState>().db_manager.pool().clone()
}

/// Queue the final transcript of a meeting with `model` (`None` = the settings' final model).
/// Returns once it is queued; the run continues in the background.
pub async fn start(app: &AppHandle, meeting_id: &str, model: Option<String>) -> Result<(), String> {
    let pool = pool_of(app);
    let settings = app.state::<VoiceService>().settings();
    let model = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| settings.final_model.clone());
    if !is_final_model(&model) {
        return Err(format!("Unknown final transcript model {:?}", model));
    }
    MeetingsRepository::get(&pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;

    let (generation, token, lock) = register(meeting_id);
    let reporter = Reporter {
        app: app.clone(),
        pool,
        meeting_id: meeting_id.to_string(),
        model,
        token,
        job_id: Mutex::new(None),
    };
    reporter.report(final_state::QUEUED, None, None, true).await;
    log::info!("Final transcript of {} queued with {}", meeting_id, reporter.model);

    tauri::async_runtime::spawn(async move {
        let _running = lock.lock().await;
        if !reporter.token.is_cancelled() {
            execute(&reporter).await;
        }
        unregister(&reporter.meeting_id, generation);
    });
    Ok(())
}

async fn execute(reporter: &Reporter) {
    let app = &reporter.app;
    let voice = app.state::<VoiceService>();
    let settings = voice.settings();
    let fail = |message: String| async move {
        log::error!("Final transcript of {} failed: {}", reporter.meeting_id, message);
        reporter.report(final_state::FAILED, Some(message), None, true).await;
    };

    let client = match voice.client() {
        Ok(client) => client,
        Err(message) => return fail(message).await,
    };
    let meeting = match MeetingsRepository::get(&reporter.pool, &reporter.meeting_id).await {
        Ok(Some(meeting)) => meeting,
        Ok(None) => return,
        Err(e) => return fail(format!("Failed to load meeting: {}", e)).await,
    };
    let Some(folder) = meeting.folder_path.as_deref().map(PathBuf::from) else {
        return fail("This meeting has no folder".to_string()).await;
    };
    let Some(audio) = find_meeting_audio(&folder) else {
        return fail(format!("No audio file in {}", folder.display())).await;
    };
    let job = FinalJob::new(&meeting, folder, audio, &reporter.model, &settings);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let run = run_final_transcript(&client, &reporter.pool, &job, &reporter.token, move |p| {
        let _ = tx.send(p);
    });
    tokio::pin!(run);
    let mut uploading_stored = false;
    let result = loop {
        tokio::select! {
            result = &mut run => break result,
            Some(progress) = rx.recv() => reporter.progress(progress, &mut uploading_stored).await,
        }
    };

    match result {
        Ok(outcome) => {
            *reporter.job_id.lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome.job_id.clone());
            log::info!(
                "Final transcript of {} done: {} turns from {:?} (job {})",
                reporter.meeting_id,
                outcome.segments_count,
                outcome.turn_source,
                outcome.job_id
            );
            reporter.report(final_state::DONE, None, Some(outcome.segments_count), true).await;
            if settings.auto_summary && outcome.segments_count > 0 && !reporter.token.is_cancelled() {
                if let Err(e) =
                    crate::summary::commands::start_summary(app, &reporter.pool, &reporter.meeting_id, None, None, None)
                        .await
                {
                    log::error!("Automatic summary of {} did not start: {}", reporter.meeting_id, e);
                }
            }
        }
        Err(FinalError::Cancelled) => log::info!("Final transcript of {} replaced or cancelled", reporter.meeting_id),
        Err(FinalError::Failed(message)) => fail(message).await,
    }
}

/// Start (or re-run) the final transcript in the background; failures to queue are logged.
pub fn start_for_meeting(app: &AppHandle, meeting_id: &str, model: Option<String>) {
    let app = app.clone();
    let meeting_id = meeting_id.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start(&app, &meeting_id, model).await {
            log::error!("Final transcript of {} not started: {}", meeting_id, e);
        }
    });
}

fn start_if_auto(app: &AppHandle, meeting_id: &str) {
    if app.state::<VoiceService>().settings().auto_final {
        start_for_meeting(app, meeting_id, None);
    } else {
        log::info!("Automatic final transcript is off; {} keeps its live transcript", meeting_id);
    }
}

/// `stop_recording` wrote `audio.mp4` and marked the meeting stopped.
pub fn on_recording_stopped(app: &AppHandle, meeting_id: &str) {
    start_if_auto(app, meeting_id);
}

/// Startup recovery merged an interrupted meeting's checkpoints into `audio.mp4`.
pub fn on_meeting_recovered(app: &AppHandle, meeting_id: &str) {
    start_if_auto(app, meeting_id);
}

/// Startup: runs that were queued or running when the app exited start again with their model.
pub async fn resume_unfinished(app: &AppHandle, pool: &SqlitePool) {
    match MeetingsRepository::final_in_progress(pool).await {
        Ok(meetings) => {
            for meeting in meetings {
                log::info!("Restarting the unfinished final transcript of {}", meeting.id);
                start_for_meeting(app, &meeting.id, meeting.final_model.clone());
            }
        }
        Err(e) => log::error!("Failed to look up unfinished final transcripts: {}", e),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// (Re)run the final transcript; returns once it is queued.
#[tauri::command]
pub async fn start_final_transcript(app: AppHandle, meeting_id: String, model: Option<String>) -> Result<(), String> {
    start(&app, &meeting_id, model).await
}

#[tauri::command]
pub async fn get_final_transcript_status(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<FinalStatus, String> {
    let meeting = MeetingsRepository::get(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;
    Ok(FinalStatus { state: meeting.final_status, model: meeting.final_model, error: meeting.final_error })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::meeting::NewMeeting;
    use crate::database::test_pool;

    fn scribe_artifact() -> Artifact {
        let job: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/peacockery-voice/tests/fixtures/b-scribe.json"
        ))
        .unwrap();
        Artifact::from_value(job["result"].clone()).unwrap()
    }

    async fn meeting(pool: &SqlitePool, folder: &Path) -> Meeting {
        MeetingsRepository::create(
            pool,
            &NewMeeting { id: "m1", title: "Call", folder_path: &folder.to_string_lossy(), status: meeting_status::STOPPED },
        )
        .await
        .unwrap();
        MeetingsRepository::get(pool, "m1").await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn store_final_replaces_final_rows_and_writes_files() {
        let pool = test_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let meeting = meeting(&pool, dir.path()).await;
        let live = NewTranscript {
            speaker: Some("Me".into()),
            text: "live line".into(),
            timestamp: None,
            audio_start_time: Some(0.0),
            audio_end_time: Some(1.0),
            duration: Some(1.0),
            sequence: Some(0),
        };
        TranscriptsRepository::insert(&pool, "m1", "live", &live).await.unwrap();
        TranscriptsRepository::insert(&pool, "m1", "final", &live).await.unwrap();

        let job = FinalJob::new(
            &meeting,
            dir.path().to_path_buf(),
            dir.path().join("audio.mp4"),
            "elevenlabs-scribe_v2",
            &VoiceSettings::default(),
        );
        assert!(job.recorded_at.is_some());
        let outcome = store_final(&pool, &job, "job_1", &scribe_artifact()).await.unwrap();
        assert!(outcome.segments_count > 1);
        assert_eq!(outcome.turn_source, TurnSource::Words);

        let (live_count, final_count) = TranscriptsRepository::source_counts(&pool, "m1").await.unwrap();
        assert_eq!(live_count, 1);
        assert_eq!(final_count as usize, outcome.segments_count);
        let rows = TranscriptsRepository::all(&pool, "m1", "final").await.unwrap();
        assert!(rows.iter().all(|r| r.speaker.as_deref().is_some_and(|s| s.starts_with("Speaker "))));
        assert!(rows.windows(2).all(|w| w[0].audio_start_time <= w[1].audio_start_time));
        assert!(rows[0].timestamp.as_deref().is_some_and(|t| t.len() == 8));

        let file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(TRANSCRIPT_FILE)).unwrap()).unwrap();
        assert_eq!(file["job_id"], "job_1");
        assert_eq!(file["turns"].as_array().unwrap().len(), outcome.segments_count);
        assert!(dir.path().join(ARTIFACT_FILE).is_file());
    }

    #[tokio::test]
    async fn energy_names_me_and_them() {
        let pool = test_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let meeting = meeting(&pool, dir.path()).await;
        // speaker_0 talks 0-4 s on the mic, speaker_1 talks 6-10 s through the system channel.
        let word = |start: f64, text: &str, speaker: &str| {
            serde_json::json!({"start_seconds": start, "end_seconds": start + 0.8, "text": text, "speaker": speaker})
        };
        let artifact = Artifact::from_value(serde_json::json!({
            "text": "Hello there friend. Hi back to you.",
            "content": {"words": {"items": [
                word(0.0, "Hello", "speaker_0"), word(1.0, "there", "speaker_0"), word(2.0, "friend.", "speaker_0"),
                word(6.0, "Hi", "speaker_1"), word(7.0, "back", "speaker_1"), word(8.0, "to", "speaker_1"),
                word(9.0, "you.", "speaker_1"),
            ]}}
        }))
        .unwrap();
        let mut track = EnergyTrack::new(100);
        for i in 0..110 {
            if i >= 55 {
                track.push_db(-70.0, -20.0);
            } else {
                track.push_db(-20.0, -90.0);
            }
        }
        std::fs::write(dir.path().join(ENERGY_FILE), serde_json::to_vec(&track).unwrap()).unwrap();
        assert!(load_energy(dir.path()).is_some());

        let job = FinalJob::new(
            &meeting,
            dir.path().to_path_buf(),
            dir.path().join("audio.mp4"),
            "elevenlabs-scribe_v2",
            &VoiceSettings::default(),
        );
        store_final(&pool, &job, "job_1", &artifact).await.unwrap();
        let rows = TranscriptsRepository::all(&pool, "m1", "final").await.unwrap();
        let named: Vec<_> = rows.iter().map(|r| (r.speaker.clone().unwrap(), r.transcript.clone())).collect();
        assert_eq!(
            named,
            vec![("Me".to_string(), "Hello there friend.".to_string()), ("Them".to_string(), "Hi back to you.".to_string())]
        );

        // An unreadable energy file falls back to Speaker N.
        std::fs::write(dir.path().join(ENERGY_FILE), b"{").unwrap();
        store_final(&pool, &job, "job_2", &artifact).await.unwrap();
        let rows = TranscriptsRepository::all(&pool, "m1", "final").await.unwrap();
        assert_eq!(rows[0].speaker.as_deref(), Some("Speaker 1"));
    }

    #[test]
    fn job_options_follow_settings() {
        let meeting = Meeting {
            id: "m1".into(),
            title: "t".into(),
            created_at: Some("2026-11-10T15:00:00+00:00".into()),
            updated_at: None,
            folder_path: None,
            status: meeting_status::IMPORTED.into(),
            duration_seconds: None,
            live_model: None,
            live_mode: None,
            realtime_session_ids: None,
            final_status: None,
            final_model: None,
            final_job_id: None,
            final_error: None,
            final_updated_at: None,
        };
        let settings = VoiceSettings { keyterms: vec!["Netask".into()], language: "auto".into(), ..VoiceSettings::default() };
        let job = FinalJob::new(&meeting, PathBuf::from("/m"), PathBuf::from("/m/audio.m4a"), "cohere-transcribe", &settings);
        assert!(!job.options.diarize);
        assert!(job.recorded_at.is_none(), "imports have no wall clock");
        assert_eq!(job.options.keyterms, vec!["Netask"]);
        assert!(job.options.request_body("k").get("language").is_none());
        let job = FinalJob::new(&meeting, PathBuf::from("/m"), PathBuf::from("/m/audio.m4a"), "elevenlabs-scribe_v2", &settings);
        assert!(job.options.diarize);
    }

    #[test]
    fn rows_carry_times_and_order() {
        let turns = vec![
            Turn { speaker: Some("Me".into()), start: 0.5, end: 2.0, text: "Hi.".into() },
            Turn { speaker: None, start: 61.0, end: 62.5, text: "Yes.".into() },
        ];
        let start = DateTime::parse_from_rfc3339("2026-11-10T09:00:00-07:00").unwrap().with_timezone(&Local);
        let rows = rows_from_turns(&turns, Some(start));
        assert_eq!(rows[1].sequence, Some(1));
        assert_eq!(rows[1].duration, Some(1.5));
        let expected = (start + chrono::Duration::seconds(61)).format("%H:%M:%S").to_string();
        assert_eq!(rows[1].timestamp.as_deref(), Some(expected.as_str()));
        assert!(rows_from_turns(&turns, None).iter().all(|r| r.timestamp.is_none()));
    }

    #[test]
    fn replacing_a_run_cancels_the_previous_one() {
        let (g1, t1, l1) = register("slot-test");
        let (g2, t2, l2) = register("slot-test");
        assert!(t1.is_cancelled() && !t2.is_cancelled());
        assert!(Arc::ptr_eq(&l1, &l2), "runs of one meeting share the lock");
        unregister("slot-test", g1);
        assert!(SLOTS.lock().unwrap().contains_key("slot-test"), "a stale run does not remove its replacement");
        unregister("slot-test", g2);
        assert!(!SLOTS.lock().unwrap().contains_key("slot-test"));
    }
}
