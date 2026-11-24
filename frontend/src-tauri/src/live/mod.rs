//! Live transcription during a recording (ARCHITECTURE.md §4–§5, §9).
//!
//! `LiveManager::start` spawns one realtime session task per channel (`split`: mic + system,
//! `mixed`: one on the mix) and returns the `LiveTaps` the audio pipeline feeds. Sessions
//! connect in the background (start never waits for them), buffer up to ~10 s of audio while
//! connecting, reconnect with backoff, and turn events into lines with the crate's
//! `LineBuilder` on the recording clock. Final lines are numbered per recording, emitted,
//! kept in the session's in-memory `LiveLineStore` and inserted as `live` rows.
//! The pipeline closing the taps is the stop signal: every session then finishes.

mod session;
pub mod taps;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Local};
use log::{info, warn};
use peacockery_voice::{Channel, LineUpdate, LiveMode, ModelFamily, VoiceClient};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

use crate::audio::recording_saver::LiveLineStore;
use crate::database::models::{transcript_source, NewTranscript};
use crate::database::repositories::transcript::TranscriptsRepository;
use crate::voice::VoiceSettings;
pub use taps::{LiveFrame, LiveTaps};

/// All sessions together get this long to finish at stop.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(20);

/// What the live sessions of one recording use.
#[derive(Debug, Clone)]
pub struct LiveConfig {
    pub meeting_id: String,
    /// Public model id (`live_model`).
    pub nova_fallback: bool,
    pub model: String,
    /// Session topology, from the recording mode.
    pub mode: LiveMode,
    /// `None` = provider detects.
    pub language: Option<String>,
    pub keyterms: Vec<String>,
    pub transcript_corrections: Vec<crate::voice::corrections::TranscriptCorrection>,
    pub me_label: String,
    pub them_label: String,
    /// Wall clock of recording time 0 (line timestamps); `RecordingManager::start` sets it
    /// when the pipeline starts.
    pub started_at: DateTime<Local>,
}

impl LiveConfig {
    pub fn from_settings(meeting_id: &str, settings: &VoiceSettings, mode: LiveMode) -> Self {
        Self {
            meeting_id: meeting_id.to_string(),
            model: settings.live_model.clone(),
            nova_fallback: settings.nova_fallback,
            mode,
            language: (settings.language != "auto").then(|| settings.language.clone()),
            keyterms: settings.keyterms.clone(),
            transcript_corrections: settings.transcript_corrections.clone(),
            me_label: settings.me_label.clone(),
            them_label: settings.them_label.clone(),
            started_at: Local::now(),
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            LiveMode::Split => "split",
            LiveMode::Mixed => "mixed",
        }
    }
}

/// Live models that separate voices; every channel of such a model is diarized, a shared
/// room microphone included.
pub fn diarize_for_model(model: &str) -> bool {
    ModelFamily::of(model) == ModelFamily::VoxtralRealtime3
        || matches!(model, "elevenlabs-scribe_v3" | "deepgram-nova-3")
}

/// Sessions for a recording: split → one per captured source, mixed → one on the mix.
pub fn channels_for(mode: LiveMode, has_mic: bool, has_system: bool) -> Vec<Channel> {
    match mode {
        LiveMode::Split => [(has_mic, Channel::Mic), (has_system, Channel::System)]
            .into_iter()
            .filter_map(|(present, channel)| present.then_some(channel))
            .collect(),
        LiveMode::Mixed => vec![Channel::Mixed],
    }
}

/// `live-transcription-status` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LiveState {
    Connecting,
    Live,
    Reconnecting,
    Failed,
    Closed,
}

/// `live-transcription-status` payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveStatus {
    pub meeting_id: String,
    pub channel: Channel,
    pub model: String,
    pub state: LiveState,
    pub message: Option<String>,
}

/// `transcript-interim` payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveInterim {
    pub meeting_id: String,
    pub channel: Channel,
    pub speaker: Option<String>,
    pub text: String,
}

/// `transcript-update` payload; also a `get_transcript_history` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveLine {
    pub meeting_id: String,
    pub sequence_id: u64,
    pub text: String,
    pub speaker: Option<String>,
    pub channel: Channel,
    /// Local wall clock `HH:MM:SS` of the line start.
    pub timestamp: String,
    pub audio_start_time: f64,
    pub audio_end_time: f64,
    pub duration: f64,
    pub is_partial: bool,
    pub source: &'static str,
    pub confidence: Option<f64>,
}

/// Where live output goes besides the store and the database (the webview in the app).
pub trait LiveSink: Send + Sync + 'static {
    fn status(&self, status: &LiveStatus);
    fn interim(&self, interim: &LiveInterim);
    fn line(&self, line: &LiveLine);
}

/// Emits the §9 events.
pub struct TauriLiveSink<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriLiveSink<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> LiveSink for TauriLiveSink<R> {
    fn status(&self, status: &LiveStatus) {
        let _ = self.app.emit("live-transcription-status", status);
    }
    fn interim(&self, interim: &LiveInterim) {
        let _ = self.app.emit("transcript-interim", interim);
    }
    fn line(&self, line: &LiveLine) {
        let _ = self.app.emit("transcript-update", line);
    }
}

/// What the meeting row keeps about its live sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSummary {
    pub model: String,
    pub mode: &'static str,
    pub session_ids: Vec<String>,
    /// Every channel transcribed all its audio on `model`.
    pub complete: bool,
}

/// State shared by the session tasks of one recording.
pub(crate) struct Shared {
    config: LiveConfig,
    client: VoiceClient,
    sink: Mutex<Option<Arc<dyn LiveSink>>>,
    store: LiveLineStore,
    sequence: AtomicU64,
    session_ids: Mutex<Vec<String>>,
    db: Mutex<Option<mpsc::UnboundedSender<NewTranscript>>>,
    /// Set when some audio was not transcribed on the configured model; shared with the
    /// taps, which set it when a session falls behind and live audio is dropped.
    incomplete: Arc<AtomicBool>,
}

impl Shared {
    fn display(&self, emit: impl FnOnce(&dyn LiveSink)) {
        if let Some(sink) = self
            .sink
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_deref()
        {
            emit(sink);
        }
    }

    fn mark_incomplete(&self, channel: Option<Channel>, reason: &str) {
        warn!("Live transcript incomplete ({:?}): {}", channel, reason);
        self.incomplete.store(true, Ordering::SeqCst);
    }

    fn status_model(
        &self,
        channel: Channel,
        state: LiveState,
        message: Option<String>,
        model: &str,
    ) {
        match &message {
            Some(message) => info!("Live {:?} session: {:?} ({})", channel, state, message),
            None => info!("Live {:?} session: {:?}", channel, state),
        }
        self.display(|sink| {
            sink.status(&LiveStatus {
                meeting_id: self.config.meeting_id.clone(),
                channel,
                model: model.to_string(),
                state,
                message,
            })
        });
    }

    fn add_session_id(&self, id: &str) {
        let mut ids = self.session_ids.lock().unwrap_or_else(|p| p.into_inner());
        if !id.is_empty() && !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
    }

    fn apply(&self, updates: Vec<LineUpdate>) {
        for update in updates {
            match update {
                LineUpdate::Preview {
                    channel,
                    speaker,
                    text,
                } => self.display(|sink| {
                    sink.interim(&LiveInterim {
                        meeting_id: self.config.meeting_id.clone(),
                        channel,
                        speaker,
                        text: crate::voice::corrections::correct_text(
                            &text,
                            &self.config.transcript_corrections,
                        ),
                    })
                }),
                LineUpdate::Final {
                    channel,
                    speaker,
                    text,
                    start,
                    end,
                } => self.final_line(channel, speaker, text, start, end),
            }
        }
    }

    fn final_line(
        &self,
        channel: Channel,
        speaker: Option<String>,
        text: String,
        start: f64,
        end: f64,
    ) {
        let text = crate::voice::corrections::correct_text(
            text.trim(),
            &self.config.transcript_corrections,
        );
        if text.is_empty() {
            return;
        }
        let end = end.max(start);
        let sequence_id = self.sequence.fetch_add(1, Ordering::SeqCst);
        let timestamp = (self.config.started_at
            + chrono::Duration::milliseconds((start * 1000.0) as i64))
        .format("%H:%M:%S")
        .to_string();
        let line = LiveLine {
            meeting_id: self.config.meeting_id.clone(),
            sequence_id,
            text,
            speaker,
            channel,
            timestamp,
            audio_start_time: start,
            audio_end_time: end,
            duration: end - start,
            is_partial: false,
            source: transcript_source::LIVE,
            confidence: None,
        };
        if let Some(db) = self.db.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            let _ = db.send(NewTranscript {
                speaker: line.speaker.clone(),
                text: line.text.clone(),
                timestamp: Some(line.timestamp.clone()),
                audio_start_time: Some(start),
                audio_end_time: Some(end),
                duration: Some(line.duration),
                sequence: Some(sequence_id as i64),
            });
        }
        self.store.push(line.clone());
        self.display(|sink| sink.line(&line));
    }
}

/// The live sessions of one recording.
pub struct LiveManager {
    shared: Arc<Shared>,
    tasks: JoinSet<()>,
    writer: Option<JoinHandle<()>>,
}

impl LiveManager {
    /// Spawn the session tasks (they connect in the background) and the DB writer. Must be
    /// called inside a tokio runtime.
    pub fn start(
        config: LiveConfig,
        client: VoiceClient,
        sink: Arc<dyn LiveSink>,
        store: LiveLineStore,
        pool: Option<SqlitePool>,
        channels: &[Channel],
    ) -> (Self, LiveTaps) {
        let incomplete: Arc<AtomicBool> = Arc::default();
        let (db, writer) = match pool {
            Some(pool) => {
                let (tx, mut rx) = mpsc::unbounded_channel::<NewTranscript>();
                let meeting_id = config.meeting_id.clone();
                let row_lost = incomplete.clone();
                let writer = tokio::spawn(async move {
                    while let Some(row) = rx.recv().await {
                        if let Err(e) = TranscriptsRepository::insert(
                            &pool,
                            &meeting_id,
                            transcript_source::LIVE,
                            &row,
                        )
                        .await
                        {
                            warn!(
                                "Live transcript incomplete: failed to store live line {:?}: {}",
                                row.sequence, e
                            );
                            row_lost.store(true, Ordering::SeqCst);
                        }
                    }
                });
                (Some(tx), Some(writer))
            }
            None => (None, None),
        };
        info!(
            "Live transcription: model {}, mode {}, sessions {:?}",
            config.model,
            config.mode_name(),
            channels
        );
        let shared = Arc::new(Shared {
            config,
            client,
            sink: Mutex::new(Some(sink)),
            store,
            sequence: AtomicU64::new(0),
            session_ids: Mutex::new(Vec::new()),
            db: Mutex::new(db),
            incomplete,
        });
        let mut tasks = JoinSet::new();
        let mut senders = Vec::new();
        for &channel in channels {
            let (tx, rx) = mpsc::channel::<LiveFrame>(taps::TAP_CHANNEL_FRAMES);
            senders.push((channel, tx));
            tasks.spawn(session::run(shared.clone(), channel, rx));
        }
        let taps = LiveTaps::new(senders, shared.incomplete.clone());
        (
            Self {
                shared,
                tasks,
                writer,
            },
            taps,
        )
    }

    /// Retire this recording's live UI while its remaining rows continue to storage.
    pub fn detach_display(&self) {
        self.shared
            .sink
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }

    /// Wait for every session to finish (the taps must be closed first), at most
    /// `STOP_TIMEOUT` in total, then for the last rows to be written.
    pub async fn finish(mut self) -> LiveSummary {
        let all = async {
            while let Some(result) = self.tasks.join_next().await {
                if let Err(e) = result {
                    self.shared
                        .mark_incomplete(None, &format!("session task failed: {e}"));
                }
            }
        };
        if tokio::time::timeout(STOP_TIMEOUT, all).await.is_err() {
            self.shared.mark_incomplete(
                None,
                &format!(
                    "sessions did not finish within {} s; abandoning them",
                    STOP_TIMEOUT.as_secs()
                ),
            );
            self.tasks.abort_all();
            while self.tasks.join_next().await.is_some() {}
        }
        self.shared
            .db
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(writer) = self.writer.take() {
            if !matches!(
                tokio::time::timeout(Duration::from_secs(10), writer).await,
                Ok(Ok(()))
            ) {
                self.shared.mark_incomplete(
                    None,
                    "live transcript writer did not finish cleanly within 10 s",
                );
            }
        }
        let session_ids = self
            .shared
            .session_ids
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        LiveSummary {
            model: self.shared.config.model.clone(),
            mode: self.shared.config.mode_name(),
            session_ids,
            complete: !self.shared.incomplete.load(Ordering::SeqCst),
        }
    }
}

/// Live transcription cannot start (no key, …): say so on every channel.
pub fn report_unavailable(
    sink: &dyn LiveSink,
    config: &LiveConfig,
    channels: &[Channel],
    message: &str,
) {
    for &channel in channels {
        sink.status(&LiveStatus {
            meeting_id: config.meeting_id.clone(),
            channel,
            model: config.model.clone(),
            state: LiveState::Failed,
            message: Some(message.to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn late_provider_line_stays_with_stopped_meeting_after_display_detaches() {
        use crate::database::manager::DatabaseManager;
        use crate::database::repositories::meeting::{MeetingsRepository, NewMeeting};
        use peacockery_voice::ClientConfig;

        struct Display(AtomicU64);
        impl LiveSink for Display {
            fn line(&self, _: &LiveLine) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            fn interim(&self, _: &LiveInterim) {}
            fn status(&self, _: &LiveStatus) {}
        }

        let dir = tempfile::tempdir().unwrap();
        let db = DatabaseManager::open(&dir.path().join("meetings.sqlite"))
            .await
            .unwrap();
        for id in ["first", "next"] {
            MeetingsRepository::create(
                db.pool(),
                &NewMeeting {
                    id,
                    title: id,
                    folder_path: id,
                    status: "recording",
                },
            )
            .await
            .unwrap();
        }
        let display = Arc::new(Display(AtomicU64::new(0)));
        let config = LiveConfig::from_settings("first", &VoiceSettings::default(), LiveMode::Mixed);
        let client = VoiceClient::new(ClientConfig::new("http://localhost", "test")).unwrap();
        let (live, _) = LiveManager::start(
            config,
            client,
            display.clone(),
            LiveLineStore::default(),
            Some(db.pool().clone()),
            &[],
        );
        live.shared
            .final_line(Channel::Mic, None, "before stop".into(), 0., 1.);
        live.detach_display();
        live.shared
            .final_line(Channel::Mic, None, "last words".into(), 1., 2.);
        assert!(live.finish().await.complete);
        assert_eq!(display.0.load(Ordering::SeqCst), 1);
        let rows = TranscriptsRepository::all(db.pool(), "first", "live")
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.transcript == "last words"));
        assert!(TranscriptsRepository::all(db.pool(), "next", "live")
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn channels_follow_mode_and_sources() {
        assert_eq!(
            channels_for(LiveMode::Split, true, true),
            vec![Channel::Mic, Channel::System]
        );
        assert_eq!(
            channels_for(LiveMode::Split, false, true),
            vec![Channel::System]
        );
        assert_eq!(
            channels_for(LiveMode::Mixed, true, false),
            vec![Channel::Mixed]
        );
    }

    #[test]
    fn config_from_settings_takes_the_recording_topology() {
        let settings = VoiceSettings {
            language: "auto".into(),
            ..VoiceSettings::default()
        };
        let config = LiveConfig::from_settings("m", &settings, LiveMode::Mixed);
        assert_eq!(config.language, None);
        assert_eq!(config.mode, LiveMode::Mixed);
        assert_eq!(config.mode_name(), "mixed");
        assert_eq!(config.model, "elevenlabs-scribe_v3");
    }

    #[test]
    fn diarization_follows_the_model() {
        assert!(diarize_for_model("elevenlabs-scribe_v3"));
        assert!(diarize_for_model(
            "mistral-voxtral-mini-transcribe-realtime-3"
        ));
        assert!(
            diarize_for_model("deepgram-nova-3"),
            "Nova fallback keeps speaker separation"
        );
        assert!(!diarize_for_model("cohere-transcribe"));
    }

    #[test]
    fn line_payload_matches_the_contract() {
        let line = LiveLine {
            meeting_id: "m".into(),
            sequence_id: 0,
            text: "Hi".into(),
            speaker: Some("Me".into()),
            channel: Channel::Mic,
            timestamp: "09:00:01".into(),
            audio_start_time: 1.0,
            audio_end_time: 2.0,
            duration: 1.0,
            is_partial: false,
            source: "live",
            confidence: None,
        };
        let value = serde_json::to_value(&line).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "meeting_id": "m", "sequence_id": 0, "text": "Hi", "speaker": "Me", "channel": "mic",
                "timestamp": "09:00:01", "audio_start_time": 1.0, "audio_end_time": 2.0, "duration": 1.0,
                "is_partial": false, "source": "live", "confidence": null
            })
        );
        let status = LiveStatus {
            meeting_id: "m".into(),
            channel: Channel::System,
            model: "x".into(),
            state: LiveState::Reconnecting,
            message: None,
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap()["state"],
            "reconnecting"
        );
    }
}
