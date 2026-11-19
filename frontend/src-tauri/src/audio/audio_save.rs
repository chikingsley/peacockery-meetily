//! Background encoding of closed audio journals into `audio.mp4`.
//!
//! Stop and startup recovery register a meeting as saving, then hand its folder to one
//! worker that encodes one meeting at a time, so two quick stops never run two encodes at
//! once. Each meeting's state is a watch channel, so a waiter that arrives late still sees
//! the result. The folder stays the durable record: a journal without `audio.mp4` means
//! the audio is still to be encoded, at the latest at the next start.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, watch};

use super::constants::find_meeting_audio;
use super::journal::{encode_journal, has_journal};
use super::pipeline::PIPELINE_SAMPLE_RATE;

#[derive(Debug, Clone, PartialEq)]
pub enum SaveState {
    Saving,
    Saved(PathBuf),
    Failed(String),
}

/// One entry per meeting saved in this process; entries are kept so late waiters see the
/// result.
static SAVES: LazyLock<Mutex<HashMap<String, watch::Sender<SaveState>>>> =
    LazyLock::new(Mutex::default);

/// How often a waiter re-checks a folder whose journal is not registered yet.
const JOURNAL_POLL: Duration = Duration::from_millis(250);

fn saves() -> MutexGuard<'static, HashMap<String, watch::Sender<SaveState>>> {
    SAVES.lock().unwrap_or_else(|p| p.into_inner())
}

fn state(meeting_id: &str) -> Option<SaveState> {
    saves().get(meeting_id).map(|s| s.borrow().clone())
}

/// Mark the meeting as saving. Call before the meeting can be seen as stopped, so the
/// final transcript, publication, quit and updates wait for the encode.
pub fn register(meeting_id: &str) {
    let mut saves = saves();
    match saves.get(meeting_id) {
        Some(sender) => {
            sender.send_replace(SaveState::Saving);
        }
        None => {
            saves.insert(
                meeting_id.to_string(),
                watch::Sender::new(SaveState::Saving),
            );
        }
    }
}

/// Publish the result of a save.
pub fn finish(meeting_id: &str, result: Result<PathBuf, String>) {
    let state = match result {
        Ok(path) => SaveState::Saved(path),
        Err(message) => SaveState::Failed(message),
    };
    saves()
        .entry(meeting_id.to_string())
        .or_insert_with(|| watch::Sender::new(SaveState::Saving))
        .send_replace(state);
}

/// The meeting is registered and its encode has not finished.
pub fn is_saving(meeting_id: &str) -> bool {
    state(meeting_id) == Some(SaveState::Saving)
}

/// Saves not finished yet, across all meetings.
pub fn pending() -> usize {
    saves()
        .values()
        .filter(|s| *s.borrow() == SaveState::Saving)
        .count()
}

/// The meeting's audio file is still to be written: registered and saving, or, when this
/// process has not registered it, its folder still holds a journal.
pub fn is_pending(meeting_id: &str, folder: &Path) -> bool {
    match state(meeting_id) {
        Some(state) => state == SaveState::Saving,
        None => has_journal(folder),
    }
}

/// The meeting's audio file. Waits for a registered save; polls while the folder holds
/// only a journal that is not registered yet; otherwise answers at once.
pub async fn wait_for_audio(meeting_id: &str, folder: &Path) -> Result<PathBuf, String> {
    loop {
        let receiver = saves().get(meeting_id).map(watch::Sender::subscribe);
        if let Some(mut receiver) = receiver {
            let state = receiver
                .wait_for(|s| *s != SaveState::Saving)
                .await
                .map(|s| s.clone());
            return match state {
                Ok(SaveState::Saved(path)) => Ok(path),
                Ok(SaveState::Failed(message)) => Err(message),
                Ok(SaveState::Saving) | Err(_) => {
                    Err("The audio save ended without a result".into())
                }
            };
        }
        if let Some(path) = find_meeting_audio(folder) {
            return Ok(path);
        }
        if !has_journal(folder) {
            return Err(format!("No audio file in {}", folder.display()));
        }
        tokio::time::sleep(JOURNAL_POLL).await;
    }
}

/// Wait until no save is pending, at most `limit`. False when saves were still running.
pub async fn wait_for_pending(limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let saving: Vec<watch::Receiver<SaveState>> = saves()
            .values()
            .filter(|s| *s.borrow() == SaveState::Saving)
            .map(watch::Sender::subscribe)
            .collect();
        if saving.is_empty() {
            return true;
        }
        for mut receiver in saving {
            let done = receiver.wait_for(|s| *s != SaveState::Saving);
            if tokio::time::timeout_at(deadline, done).await.is_err() {
                return false;
            }
        }
    }
}

type OnDone = Box<dyn FnOnce(&Result<PathBuf, String>) + Send>;

struct Encode {
    meeting_id: String,
    folder: PathBuf,
    on_done: OnDone,
}

static WORKER: LazyLock<mpsc::UnboundedSender<Encode>> = LazyLock::new(|| {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Encode>();
    tauri::async_runtime::spawn(async move {
        while let Some(job) = receiver.recv().await {
            let result = encode(job.folder).await;
            match &result {
                Ok(path) => log::info!("Audio of {} saved: {}", job.meeting_id, path.display()),
                Err(message) => log::error!("Audio of {} not saved: {}", job.meeting_id, message),
            }
            finish(&job.meeting_id, result.clone());
            (job.on_done)(&result);
        }
    });
    sender
});

async fn encode(folder: PathBuf) -> Result<PathBuf, String> {
    match tokio::task::spawn_blocking(move || encode_journal(&folder, PIPELINE_SAMPLE_RATE)).await
    {
        Ok(result) => result.map_err(|e| format!("{e:#}")),
        Err(e) => Err(format!(
            "The audio encode stopped ({e}). The recording is safe on disk and will be encoded at the next start."
        )),
    }
}

/// Queue the encode of a registered meeting's journal. `on_done` runs on the worker once
/// the result is published.
pub fn queue(
    meeting_id: &str,
    folder: PathBuf,
    on_done: impl FnOnce(&Result<PathBuf, String>) + Send + 'static,
) {
    let job = Encode {
        meeting_id: meeting_id.to_string(),
        folder,
        on_done: Box::new(on_done),
    };
    if let Err(mpsc::error::SendError(job)) = WORKER.send(job) {
        let result = Err("The audio worker is not running".to_string());
        finish(&job.meeting_id, result.clone());
        (job.on_done)(&result);
    }
}

/// Emit an encode result: `recording-audio`, plus `recording-error` when it failed.
pub fn report(app: &AppHandle, meeting_id: &str, result: &Result<PathBuf, String>) {
    let (state, message) = match result {
        Ok(_) => ("saved", None),
        Err(message) => ("failed", Some(message.as_str())),
    };
    let _ = app.emit(
        "recording-audio",
        serde_json::json!({ "meeting_id": meeting_id, "state": state, "message": message }),
    );
    if let Some(message) = message {
        let _ = app.emit("recording-error", serde_json::json!({ "message": message }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::journal::AudioJournal;
    use crate::audio::recording_state::{AudioChunk, DeviceType};

    fn journal_folder(dir: &Path) -> PathBuf {
        let folder = dir.join("Meeting");
        let mut journal = AudioJournal::create(&folder, PIPELINE_SAMPLE_RATE).unwrap();
        journal
            .add_chunk(AudioChunk {
                data: vec![0.1; PIPELINE_SAMPLE_RATE as usize],
                device_type: DeviceType::Microphone,
            })
            .unwrap();
        journal.close().unwrap();
        folder
    }

    #[tokio::test]
    async fn a_registered_save_resolves_saved() {
        let dir = tempfile::tempdir().unwrap();
        let folder = journal_folder(dir.path());
        register("save-saved");
        assert!(is_saving("save-saved"));
        assert!(is_pending("save-saved", &folder));
        assert!(pending() >= 1);
        let waiter = tokio::spawn({
            let folder = folder.clone();
            async move { wait_for_audio("save-saved", &folder).await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());
        let audio = folder.join("audio.mp4");
        finish("save-saved", Ok(audio.clone()));
        assert_eq!(waiter.await.unwrap(), Ok(audio.clone()));
        assert!(!is_saving("save-saved"));
        // A late waiter sees the result too.
        assert_eq!(wait_for_audio("save-saved", &folder).await, Ok(audio));
    }

    #[tokio::test]
    async fn a_failed_save_reports_its_error_and_is_not_pending() {
        let dir = tempfile::tempdir().unwrap();
        let folder = journal_folder(dir.path());
        register("save-failed");
        finish("save-failed", Err("disk full".into()));
        assert_eq!(
            wait_for_audio("save-failed", &folder).await,
            Err("disk full".into())
        );
        // The journal stays for the next start, but nothing is saving it now.
        assert!(has_journal(&folder));
        assert!(!is_pending("save-failed", &folder));
    }

    #[tokio::test]
    async fn an_existing_file_or_nothing_answers_at_once() {
        let dir = tempfile::tempdir().unwrap();
        assert!(wait_for_audio("save-none", dir.path())
            .await
            .unwrap_err()
            .contains("No audio file"));
        let audio = dir.path().join("audio.m4a");
        std::fs::write(&audio, b"x").unwrap();
        assert_eq!(wait_for_audio("save-existing", dir.path()).await, Ok(audio));
        assert!(!is_pending("save-existing", dir.path()));
    }

    #[tokio::test]
    async fn an_unregistered_journal_waits_for_its_registration() {
        let dir = tempfile::tempdir().unwrap();
        let folder = journal_folder(dir.path());
        assert!(is_pending("save-journal", &folder));
        let waiter = tokio::spawn({
            let folder = folder.clone();
            async move { wait_for_audio("save-journal", &folder).await }
        });
        tokio::time::sleep(JOURNAL_POLL * 2).await;
        assert!(
            !waiter.is_finished(),
            "a journal alone keeps the waiter waiting"
        );
        register("save-journal");
        finish("save-journal", Err("encode failed".into()));
        assert_eq!(waiter.await.unwrap(), Err("encode failed".into()));
    }

    #[tokio::test]
    async fn waiting_for_pending_saves_is_capped() {
        register("save-pending");
        assert!(!wait_for_pending(Duration::from_millis(50)).await);
        let finisher = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            finish("save-pending", Err("stopped".into()));
        });
        assert!(wait_for_pending(Duration::from_secs(10)).await);
        finisher.await.unwrap();
    }

    #[tokio::test]
    async fn the_worker_reports_a_missing_journal() {
        let dir = tempfile::tempdir().unwrap();
        register("save-worker-missing");
        let (done, reported) = tokio::sync::oneshot::channel();
        queue(
            "save-worker-missing",
            dir.path().to_path_buf(),
            move |result| {
                let _ = done.send(result.clone());
            },
        );
        let reported = reported.await.unwrap().unwrap_err();
        assert!(reported.contains("No audio journal"));
        assert_eq!(
            wait_for_audio("save-worker-missing", dir.path()).await,
            Err(reported)
        );
    }

    #[tokio::test]
    async fn the_worker_encodes_a_closed_journal() {
        if crate::audio::ffmpeg::find_ffmpeg_path().is_none() {
            eprintln!("ffmpeg not available; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let first = journal_folder(dir.path());
        let second_dir = tempfile::tempdir().unwrap();
        let second = journal_folder(second_dir.path());
        register("save-worker-1");
        register("save-worker-2");
        queue("save-worker-1", first.clone(), |_| {});
        queue("save-worker-2", second.clone(), |_| {});
        let audio = wait_for_audio("save-worker-2", &second).await.unwrap();
        assert_eq!(audio, second.join("audio.mp4"));
        // Encodes run in queue order, one at a time.
        assert_eq!(
            state("save-worker-1"),
            Some(SaveState::Saved(first.join("audio.mp4")))
        );
        assert!(!has_journal(&first) && !has_journal(&second));
    }
}
