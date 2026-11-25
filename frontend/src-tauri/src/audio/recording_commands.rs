// audio/recording_commands.rs
//
// Tauri commands for recording. Rust owns the whole meeting lifecycle: start creates the
// meeting row (status 'recording') and its folder; stop drains the live transcript, closes
// the audio journal, marks the meeting 'stopped', hands it to the final transcript and
// queues the `audio.mp4` encode in the background (`audio_save`).

use log::{error, info, warn};
use serde::Serialize;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use super::device_monitor::MicDisconnected;
use super::recording_manager::{LiveSetup, RecordingManager};
use super::recording_saver::MeetingTarget;
use super::stream::{SystemSource, CHROME_APP_DEVICE, CHROME_APP_LABEL};
use super::{
    default_input_device, default_output_device, parse_audio_device, AudioDevice, DeviceType,
};
use crate::database::models::meeting_status;
use crate::database::repositories::meeting::{MeetingsRepository, NewMeeting};
use crate::live::{LiveConfig, LiveLine, TauriLiveSink};
use crate::recording_context::{CalendarEvent, RecordingContext, RecordingMode};
use crate::state::AppState;
use crate::voice::VoiceService;

// ============================================================================
// GLOBAL STATE
// ============================================================================

static IS_RECORDING: AtomicBool = AtomicBool::new(false);

/// True for the whole `stop_recording` tail. The mic-disconnect fallback checks it so a
/// fallback queued before Stop does not retry against a taken manager.
static IS_RECORDING_STOPPING: AtomicBool = AtomicBool::new(false);

static RECORDING_MANAGER: Mutex<Option<RecordingManager>> = Mutex::new(None);

/// Serializes start and stop so they never interleave.
pub(crate) static LIFECYCLE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Recording is live and not being torn down.
fn recording_live() -> bool {
    IS_RECORDING.load(Ordering::SeqCst) && !IS_RECORDING_STOPPING.load(Ordering::SeqCst)
}

fn lock_manager() -> std::sync::MutexGuard<'static, Option<RecordingManager>> {
    RECORDING_MANAGER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Recording is live AND the global manager is still the session `s` belongs to.
/// Never call while holding `RECORDING_MANAGER` (non-reentrant).
fn session_live(s: &Arc<super::RecordingState>) -> bool {
    recording_live()
        && lock_manager()
            .as_ref()
            .is_some_and(|m| Arc::ptr_eq(m.get_state(), s))
}

/// Clears `IS_RECORDING_STOPPING` on every exit path of the stop tail.
struct StoppingGuard;
impl StoppingGuard {
    fn new() -> Self {
        IS_RECORDING_STOPPING.store(true, Ordering::SeqCst);
        StoppingGuard
    }
}
impl Drop for StoppingGuard {
    fn drop(&mut self) {
        IS_RECORDING_STOPPING.store(false, Ordering::SeqCst);
    }
}

// ============================================================================
// PUBLIC TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct RecordingStarted {
    pub meeting_id: String,
    pub folder_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordingStopped {
    pub meeting_id: String,
    pub folder_path: String,
}

// ============================================================================
// DEVICE RESOLUTION
// ============================================================================

/// A selected microphone must still exist. Only a default-device selection follows Windows.
fn resolve_mic_or_default<R: Runtime>(
    app: &AppHandle<R>,
    requested_name: Option<&str>,
) -> Result<Option<Arc<AudioDevice>>, String> {
    if let Some(name) = requested_name {
        let device = parse_audio_device(name).map_err(|e| e.to_string())?;
        return Ok(Some(Arc::new(device)));
    }
    match default_input_device() {
        Ok(device) => Ok(Some(Arc::new(device))),
        Err(e) => {
            warn!("Default microphone unavailable: {e}");
            let _ = app.emit("mic-unavailable", serde_json::json!({}));
            Ok(None)
        }
    }
}

/// Resolve the system source: requested (parsed; `app:chrome` is the Chrome process capture,
/// which fails here when Chrome is not running) → default render endpoint → none.
pub fn resolve_system_or_default(
    requested_name: Option<&str>,
) -> Result<Option<SystemSource>, String> {
    if let Some(name) = requested_name {
        match parse_audio_device(name) {
            Ok(device) => {
                info!("Using requested system audio: '{}'", device.name);
                let source = SystemSource::for_device(Arc::new(device));
                source.check().map_err(|e| e.to_string())?;
                return Ok(Some(source));
            }
            Err(e) => warn!(
                "Requested system audio '{}' not available: {} — using the default",
                name, e
            ),
        }
    }
    match default_output_device() {
        Ok(device) => {
            info!("Using default system audio: '{}'", device.name);
            Ok(Some(SystemSource::Endpoint(Arc::new(device))))
        }
        Err(e) => {
            warn!(
                "No system audio available: {} — recording microphone only",
                e
            );
            Ok(None)
        }
    }
}

// ============================================================================
// DEVICE COMMANDS
// ============================================================================

#[tauri::command]
pub async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    let mut devices = super::list_audio_devices()
        .await
        .map_err(|e| format!("Failed to list audio devices: {}", e))?;
    if app_loopback::is_supported() {
        devices.push(AudioDevice::with_label(
            CHROME_APP_DEVICE,
            DeviceType::Output,
            CHROME_APP_LABEL,
        ));
    }
    Ok(devices)
}

#[tauri::command]
pub async fn start_audio_level_monitoring(
    app: AppHandle,
    device_names: Vec<String>,
) -> Result<(), String> {
    if IS_RECORDING.load(Ordering::SeqCst) {
        return Err("Stop the recording before testing devices".to_string());
    }
    super::level_monitor::start_monitoring(app, device_names)
        .await
        .map_err(|e| format!("Failed to start audio level monitoring: {}", e))
}

#[tauri::command]
pub async fn stop_audio_level_monitoring() -> Result<(), String> {
    super::level_monitor::stop_monitoring();
    Ok(())
}

// ============================================================================
// RECORDING COMMANDS
// ============================================================================

/// Start recording a new meeting: creates the meeting row (status `recording`) and its
/// folder, then starts capture. A `None` device means the system default.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_recording_with_devices_and_meeting(
    app: AppHandle,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: String,
    recording_mode: RecordingMode,
    calendar_event: Option<CalendarEvent>,
    publish_to_cica: bool,
    brief_key: Option<String>,
    contact: Option<crate::briefs::CallContact>,
) -> Result<RecordingStarted, String> {
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    if IS_RECORDING.load(Ordering::SeqCst) {
        return Err("Recording already in progress".to_string());
    }
    let result = start_recording(
        &app,
        mic_device_name,
        system_device_name,
        meeting_name,
        recording_mode,
        calendar_event,
        publish_to_cica,
        brief_key,
        contact,
    )
    .await;
    // Every outcome, failures included, puts the tray back in step with the real state.
    crate::tray::update_tray_menu(&app);
    result
}

#[allow(clippy::too_many_arguments)]
async fn start_recording(
    app: &AppHandle,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: String,
    mode: RecordingMode,
    calendar_event: Option<CalendarEvent>,
    publish_to_cica: bool,
    brief_key: Option<String>,
    contact: Option<crate::briefs::CallContact>,
) -> Result<RecordingStarted, String> {
    info!(
        "Starting recording: mic={:?}, system={:?}, meeting={:?}, mode={:?}",
        mic_device_name, system_device_name, meeting_name, mode
    );
    let title = meeting_name.trim().to_string();
    if title.is_empty() {
        return Err("A meeting title is required".into());
    }
    let _ = app.emit("recording-starting", serde_json::json!({}));
    crate::tray::set_tray_state(app, crate::tray::RecordingState::Starting);

    let microphone_device = resolve_mic_or_default(app, mic_device_name.as_deref())?;
    let system_source = if mode.captures_system() {
        resolve_system_or_default(system_device_name.as_deref())?
    } else {
        None
    };
    if !mode.captures_system() && microphone_device.is_none() {
        return Err("In-person recording needs an available microphone".into());
    }
    // The device test holds the same devices open.
    super::level_monitor::stop_monitoring();

    let base_folder = super::recording_preferences::recordings_folder(app)?;
    let folder = super::audio_processing::create_meeting_folder(&base_folder, &title)
        .map_err(|e| format!("Failed to create meeting folder: {}", e))?;
    let folder_path = folder.to_string_lossy().to_string();

    let meeting_id = format!("meeting-{}", uuid::Uuid::new_v4());
    let context = RecordingContext {
        brief_key: brief_key.or_else(|| calendar_event.as_ref().map(|e| format!("event:{}", e.id))),
        mode,
        calendar_event,
    };
    if let Err(e) = context.save(&folder) {
        remove_folder(&folder);
        return Err(format!("Could not save recording options: {e}"));
    }
    let pool = app.state::<AppState>().db_manager.pool().clone();
    if let Err(e) = MeetingsRepository::create(
        &pool,
        &NewMeeting {
            id: &meeting_id,
            title: &title,
            folder_path: &folder_path,
            status: meeting_status::RECORDING,
        },
    )
    .await
    {
        remove_folder(&folder);
        return Err(format!("Failed to create meeting: {}", e));
    }

    if let Some(key) = context
        .brief_key
        .as_ref()
        .filter(|k| k.starts_with("event:"))
    {
        if let Err(error) = sqlx::query("INSERT INTO meeting_reviews(meeting_id,brief_key) VALUES(?,?) ON CONFLICT(meeting_id) DO UPDATE SET brief_key=excluded.brief_key")
            .bind(&meeting_id).bind(key).execute(&pool).await {
            let _ = MeetingsRepository::delete(&pool, &meeting_id).await;
            remove_folder(&folder);
            return Err(format!("Could not save calendar link: {error}"));
        }
    }
    if let Some(key) = context
        .brief_key
        .as_deref()
        .filter(|k| k.starts_with("task:"))
    {
        if let Err(e) = sqlx::query(
            "INSERT INTO call_destinations(meeting_id,task_key,name,number) VALUES(?,?,?,?)",
        )
        .bind(&meeting_id)
        .bind(key)
        .bind(contact.as_ref().map(|c| &c.name))
        .bind(contact.as_ref().map(|c| &c.number))
        .execute(&pool)
        .await
        {
            let _ = MeetingsRepository::delete(&pool, &meeting_id).await;
            remove_folder(&folder);
            return Err(format!("Could not save call destination: {e}"));
        }
    }
    if publish_to_cica {
        if let Err(error) = crate::publication::enqueue(&pool, &meeting_id).await {
            let _ = sqlx::query("DELETE FROM meetings WHERE id=?")
                .bind(&meeting_id)
                .execute(&pool)
                .await;
            remove_folder(&folder);
            return Err(format!("Could not save publishing preference: {error}"));
        }
    }
    let target = MeetingTarget {
        meeting_id: meeting_id.clone(),
        meeting_name: title,
        folder: folder.clone(),
    };
    let live = live_setup(app, &meeting_id, &pool, &context);
    let app_for_levels = app.clone();
    let levels = Box::new(move |levels: super::levels::AudioLevels| {
        let _ = app_for_levels.emit("audio-levels", levels);
    });
    let mut manager =
        match RecordingManager::start(target, microphone_device, system_source, live, levels).await
        {
            Ok(manager) => manager,
            Err(e) => {
                error!("Failed to start recording: {:#}", e);
                if let Err(db_error) = MeetingsRepository::delete(&pool, &meeting_id).await {
                    error!(
                        "Failed to delete meeting {} after start error: {}",
                        meeting_id, db_error
                    );
                }
                remove_folder(&folder);
                return Err(format!("Failed to start recording: {}", e));
            }
        };

    let app_for_error = app.clone();
    manager.set_error_callback(move |error| {
        let _ = app_for_error.emit(
            "recording-error",
            serde_json::json!({ "message": error.user_message() }),
        );
    });
    let device_event_receiver = manager.take_device_event_receiver();
    let session = manager.get_state().clone();
    *lock_manager() = Some(manager);
    IS_RECORDING.store(true, Ordering::SeqCst);
    crate::meeting_focus::changed(app);
    session.expect_system_audio(mode.captures_system());
    if let Some(end) = context
        .calendar_event
        .as_ref()
        .and_then(|e| chrono::DateTime::parse_from_rfc3339(&e.end).ok())
    {
        let wait = (end.to_utc() - chrono::Utc::now())
            .to_std()
            .unwrap_or_default();
        let app = app.clone();
        let session = Arc::downgrade(&session);
        tokio::spawn(async move {
            tokio::time::sleep(wait + std::time::Duration::from_secs(60)).await;
            if session.upgrade().is_some_and(|s| session_live(&s)) {
                app.dialog().message("The scheduled meeting has ended. Meetily is still recording. Stop recording in Meetily when the call is over.").title("Meeting time has ended").show(|_| {});
            }
        });
    }
    if let Some(receiver) = device_event_receiver {
        spawn_device_event_processor(app.clone(), receiver, session);
    }

    let started = RecordingStarted {
        meeting_id,
        folder_path,
    };
    let _ = app.emit("recording-started", &started);
    info!("Recording started for meeting {}", started.meeting_id);
    Ok(started)
}

/// Live transcription for a new recording, per the voice settings (`None` when disabled).
fn live_setup(
    app: &AppHandle,
    meeting_id: &str,
    pool: &sqlx::SqlitePool,
    context: &RecordingContext,
) -> Option<LiveSetup> {
    let service = app.state::<VoiceService>();
    let settings = service.settings();
    if !settings.live_enabled {
        info!("Live transcription is off");
        return None;
    }
    let mut config = LiveConfig::from_settings(meeting_id, &settings, context.mode.live_mode());
    context.add_event_names(&mut config.keyterms);
    Some(LiveSetup {
        config,
        client: service.client(),
        sink: Arc::new(TauriLiveSink::new(app.clone())),
        pool: Some(pool.clone()),
    })
}

fn remove_folder(folder: &Path) {
    if let Err(e) = std::fs::remove_dir_all(folder) {
        warn!("Failed to remove {}: {}", folder.display(), e);
    }
}

/// Close local capture and persist the stop before releasing the recorder. The registered
/// save covers live-session shutdown, automatic work registration and encoding, including
/// when quit or an update follows Stop immediately.
#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Result<RecordingStopped, String> {
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    let Some(mut manager) = lock_manager().take() else {
        return Err("No recording in progress".to_string());
    };
    let stopping = StoppingGuard::new();
    info!("Stopping recording");
    let _ = app.emit(
        "recording-stopping",
        serde_json::json!({ "meeting_id": manager.target().meeting_id }),
    );
    crate::tray::set_tray_state(&app, crate::tray::RecordingState::Stopping);

    // Only capture and the local journal need to finish in the recorder lifecycle lock.
    let stopped = manager.stop_and_save().await;
    let target = manager.target().clone();
    drop(manager);
    // A journal that failed to close (disk full, I/O error) still holds every second synced
    // before the failure; it is saved like a closed one.
    let saving = stopped.journal.is_ok() || super::journal::has_journal(&target.folder);
    // Registered before the meeting reads as stopped and before the recording flag and the
    // lifecycle lock are released, so every reader sees either the recording or the save.
    if saving {
        super::audio_save::register(&target.meeting_id);
    }
    IS_RECORDING.store(false, Ordering::SeqCst);

    if let Some(live) = &stopped.live {
        live.detach_display();
    }
    let pool = app.state::<AppState>().db_manager.pool().clone();
    if let Err(e) =
        MeetingsRepository::mark_stopped(&pool, &target.meeting_id, stopped.duration_seconds).await
    {
        error!(
            "Failed to mark meeting {} stopped: {}",
            target.meeting_id, e
        );
    }

    let folder_path = target.folder.to_string_lossy().to_string();
    drop(stopping);
    crate::meeting_focus::changed(&app);
    let _ = app.emit(
        "recording-stopped",
        serde_json::json!({
            "meeting_id": target.meeting_id,
            "folder_path": folder_path,
            "meeting_name": target.meeting_name,
        }),
    );
    crate::tray::update_tray_menu(&app);

    if let Err(e) = &stopped.journal {
        let message = if saving {
            format!(
                "The last seconds of the recording could not be written to disk ({:#}); the rest is being saved.",
                e
            )
        } else {
            format!(
                "Recording stopped but the audio could not be saved: {:#}",
                e
            )
        };
        error!("{}", message);
        let _ = app.emit("recording-error", serde_json::json!({ "message": message }));
    }
    let meeting_id = target.meeting_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(live) = stopped.live {
            let live = live.finish().await;
            let ids = serde_json::to_string(&live.session_ids).unwrap_or_else(|_| "[]".to_string());
            if let Err(e) = MeetingsRepository::set_live_info(
                &pool,
                &target.meeting_id,
                &live.model,
                live.mode,
                &ids,
                live.complete,
            )
            .await
            {
                error!(
                    "Failed to store live session info for {}: {}",
                    target.meeting_id, e
                );
            }
        }
        if saving {
            crate::final_transcript::on_recording_stopped(&app, &target.meeting_id).await;
            let id = target.meeting_id.clone();
            super::audio_save::queue(&target.meeting_id, target.folder, move |result| {
                super::audio_save::report(&app, &id, result);
            });
        }
    });
    Ok(RecordingStopped {
        meeting_id,
        folder_path,
    })
}

/// The stop tail is running (capture ended, live transcript and audio journal being finished).
pub fn is_recording_stopping() -> bool {
    IS_RECORDING_STOPPING.load(Ordering::SeqCst)
}

/// Recording is in progress (including paused).
pub fn is_recording_active() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

/// Recording is in progress and paused.
pub fn is_recording_paused() -> bool {
    lock_manager().as_ref().is_some_and(|m| m.is_paused())
}

#[tauri::command]
pub async fn pause_recording(app: AppHandle) -> Result<(), String> {
    if !is_recording_active() {
        return Err("No recording is currently active".to_string());
    }
    {
        let guard = lock_manager();
        let manager = guard
            .as_ref()
            .ok_or_else(|| "No recording manager found".to_string())?;
        manager.pause_recording().map_err(|e| e.to_string())?;
    }
    let _ = app.emit("recording-paused", serde_json::json!({}));
    crate::tray::update_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub async fn resume_recording(app: AppHandle) -> Result<(), String> {
    if !is_recording_active() {
        return Err("No recording is currently active".to_string());
    }
    {
        let guard = lock_manager();
        let manager = guard
            .as_ref()
            .ok_or_else(|| "No recording manager found".to_string())?;
        manager.resume_recording().map_err(|e| e.to_string())?;
    }
    let _ = app.emit("recording-resumed", serde_json::json!({}));
    crate::tray::update_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_recording_state() -> serde_json::Value {
    let is_recording = is_recording_active();
    let is_stopping = is_recording_stopping();
    match lock_manager().as_ref() {
        Some(manager) => serde_json::json!({
            "is_recording": is_recording,
            "is_stopping": is_stopping,
            "is_paused": manager.is_paused(),
            "is_active": manager.is_active(),
            "meeting_id": manager.target().meeting_id,
            "recording_duration": manager.get_recording_duration(),
            "active_duration": manager.get_active_recording_duration(),
            "total_pause_duration": manager.get_total_pause_duration(),
            "current_pause_duration": manager.get_current_pause_duration(),
            "local_progress": manager.recording_progress()
        }),
        // Also the stop tail: the manager is taken while the recording is being finished.
        None => serde_json::json!({
            "is_recording": is_recording,
            "is_stopping": is_stopping,
            "is_paused": false,
            "is_active": false,
            "meeting_id": null,
            "recording_duration": null,
            "active_duration": null,
            "total_pause_duration": 0.0,
            "current_pause_duration": null
        }),
    }
}

/// Live lines of the current recording session (page-reload sync).
#[tauri::command]
pub async fn get_transcript_history() -> Vec<LiveLine> {
    lock_manager()
        .as_ref()
        .map(|m| m.live_lines())
        .unwrap_or_default()
}

// ============================================================================
// MIC HOT-SWAP (disconnect recovery)
// ============================================================================

/// Perform mic hot-swap using phased locking — never holds RECORDING_MANAGER during I/O
/// except the brief mic-stream stop in Phase 1.
/// If CPAL hangs during stream creation, only this task blocks; stop flow stays unblocked.
async fn perform_mic_hot_swap_task<R: Runtime>(
    new_device_name: String,
    session: &Arc<super::RecordingState>,
    app: AppHandle<R>,
) -> Result<(), String> {
    info!("[HOT_SWAP] Starting mic hot-swap to '{}'", new_device_name);

    match do_mic_swap(&new_device_name, session).await {
        Ok(()) => {
            info!("[HOT_SWAP] Mic switched to '{}'", new_device_name);
            let _ = app.emit(
                "mic-device-switched",
                serde_json::json!({
                    "device_name": new_device_name
                }),
            );
            Ok(())
        }
        Err(e) => {
            if !session_live(session) {
                return Err(e);
            }
            warn!("[HOT_SWAP] First attempt failed: {} — retrying in 500ms", e);
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            match do_mic_swap(&new_device_name, session).await {
                Ok(()) => {
                    info!("[HOT_SWAP] Mic switched to '{}' on retry", new_device_name);
                    let _ = app.emit(
                        "mic-device-switched",
                        serde_json::json!({
                            "device_name": new_device_name
                        }),
                    );
                    Ok(())
                }
                Err(e) => {
                    error!("[HOT_SWAP] Mic swap failed after retry: {}", e);
                    if session_live(session) {
                        let _ = app.emit(
                            "mic-swap-failed",
                            serde_json::json!({
                                "error": e,
                                "device_name": new_device_name
                            }),
                        );
                    }
                    Err(e)
                }
            }
        }
    }
}

/// Phased mic swap — lock is never held during async I/O.
async fn do_mic_swap(
    device_name: &str,
    session: &Arc<super::RecordingState>,
) -> Result<(), String> {
    // Phase 1: Lock briefly — verify identity, take old stream OUT (no teardown under lock)
    let old_mic = {
        let mut guard = lock_manager();
        let manager = guard
            .as_mut()
            .ok_or_else(|| "Recording manager not available".to_string())?;
        if !manager.is_recording() {
            return Err("Recording stopped — aborting mic hot-swap".to_string());
        }
        if !Arc::ptr_eq(manager.get_state(), session) {
            return Err("Session changed before hot-swap — aborting".to_string());
        }
        manager.take_mic_stream_for_swap()
    }; // lock released

    // Tear down the dead mic OUTSIDE the lock — cpal stop()/drop on a
    // disconnected BT device can stall; doing it under RECORDING_MANAGER
    // would freeze stop_recording.
    // Non-fatal: the replacement stream is created next regardless, so a
    // teardown error/stall on the already-dead device must not abort the swap.
    if let Some(s) = old_mic {
        if let Err(e) = s.stop() {
            warn!(
                "[HOT_SWAP] Failed to stop old mic stream (proceeding): {}",
                e
            );
        }
    }

    // Phase 2: Async I/O WITHOUT lock — may be slow, that's OK
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Build the AudioDevice directly from the name — the caller
    // already resolved the approved input via
    // default_input_device(). Skipping list_audio_devices() here avoids a
    // full cpal enumeration on the exact BT-transition hot path where it's
    // known to hang 100+ s. The real device
    // validation happens inside AudioStream::create → get_device_and_config
    // which does a targeted host.input_devices() lookup by name.
    let device_arc =
        std::sync::Arc::new(AudioDevice::new(device_name.to_string(), DeviceType::Input));

    info!(
        "[HOT_SWAP] Creating new mic stream for '{}' (lock released)",
        device_name
    );
    let new_stream = super::stream::AudioStream::create(
        device_arc.clone(),
        session.clone(),
        super::recording_state::DeviceType::Microphone,
    )
    .await
    .map_err(|e| format!("Failed to create mic stream: {}", e))?;

    // Phase 3: Lock briefly — install ONLY if still the same session
    {
        let mut guard = lock_manager();
        match guard.as_mut() {
            Some(manager) if Arc::ptr_eq(manager.get_state(), session) => {
                manager.set_mic_stream_after_swap(new_stream, device_arc);
                info!("[HOT_SWAP] Mic hot-swap to '{}' completed", device_name);
            }
            Some(_) => {
                return Err(
                    "Session changed during hot-swap — discarding stale mic stream".to_string(),
                );
            }
            None => {
                return Err("Recording manager gone during hot-swap".to_string());
            }
        }
    } // lock released

    Ok(())
}

/// Serial per recording. Each replacement requires consent and a decline lasts for this session.
fn spawn_device_event_processor<R: Runtime>(
    app: AppHandle<R>,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<MicDisconnected>,
    session: Arc<super::RecordingState>,
) {
    tokio::spawn(async move {
        let mut declined = std::collections::HashSet::new();
        let mut failures = 0;
        while let Some(MicDisconnected { device_name }) = receiver.recv().await {
            if !session_live(&session) {
                break;
            }
            let Ok(device) = default_input_device() else {
                continue;
            };
            let name = device.name;
            if name == device_name || declined.contains(&name) {
                continue;
            }
            let (answer, response) = tokio::sync::oneshot::channel();
            app.dialog()
                .message(format!("{device_name} disconnected. Switch this recording to {name}? This microphone may pick up the room."))
                .title("Microphone disconnected")
                .buttons(MessageDialogButtons::OkCancelCustom(format!("Use {name}"), "Keep microphone off".into()))
                .show(move |accepted| { let _ = answer.send(accepted); });
            let accepted = response.await.unwrap_or(false);
            while receiver.try_recv().is_ok() {}
            if !session_live(&session) {
                break;
            }
            if !accepted {
                declined.insert(name);
                continue;
            }
            if perform_mic_hot_swap_task(name, &session, app.clone())
                .await
                .is_ok()
            {
                failures = 0;
            } else {
                failures += 1;
                if failures >= 3 {
                    let _ = app.emit(
                        "mic-recovery-exhausted",
                        serde_json::json!({ "device_name": device_name }),
                    );
                    break;
                }
            }
        }
    });
}
