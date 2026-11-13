// audio/recording_commands.rs
//
// Tauri commands for recording. Rust owns the whole meeting lifecycle: start creates the
// meeting row (status 'recording') and its folder; stop drains and saves `audio.mp4`,
// marks the meeting 'stopped' and hands it to the final transcript.

use log::{debug, error, info, warn};
use serde::Serialize;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::device_monitor::{DeviceEvent, DeviceMonitorType};
use super::recording_manager::{LiveSetup, RecordingManager};
use super::recording_saver::MeetingTarget;
use super::stream::{SystemSource, CHROME_APP_DEVICE, CHROME_APP_LABEL};
use super::{default_input_device, default_output_device, parse_audio_device, AudioDevice, DeviceType};
use crate::live::{LiveConfig, LiveLine, TauriLiveSink};
use crate::voice::VoiceService;
use crate::database::models::meeting_status;
use crate::database::repositories::meeting::{MeetingsRepository, NewMeeting};
use crate::state::AppState;

// ============================================================================
// GLOBAL STATE
// ============================================================================

static IS_RECORDING: AtomicBool = AtomicBool::new(false);

/// True for the whole `stop_recording` tail. The mic-disconnect fallback checks it so a
/// fallback queued before Stop does not retry against a taken manager.
static IS_RECORDING_STOPPING: AtomicBool = AtomicBool::new(false);

static RECORDING_MANAGER: Mutex<Option<RecordingManager>> = Mutex::new(None);

/// Serializes start and stop so they never interleave.
static LIFECYCLE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Recording is live and not being torn down.
fn recording_live() -> bool {
    IS_RECORDING.load(Ordering::SeqCst) && !IS_RECORDING_STOPPING.load(Ordering::SeqCst)
}

fn lock_manager() -> std::sync::MutexGuard<'static, Option<RecordingManager>> {
    RECORDING_MANAGER.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
    /// `None` when the audio could not be saved (reported through `recording-error`).
    pub audio_path: Option<String>,
}

// ============================================================================
// DEVICE RESOLUTION
// ============================================================================

/// Resolve the microphone: requested device (if it enumerates) → system default → none
/// (system-audio-only). Emits `mic-device-switched` when a requested mic was replaced by
/// the default and `mic-unavailable` when there is no mic at all.
fn resolve_mic_or_default<R: Runtime>(app: &AppHandle<R>, requested_name: Option<&str>) -> Option<Arc<AudioDevice>> {
    use cpal::traits::{DeviceTrait, HostTrait};

    if let Some(name) = requested_name {
        match parse_audio_device(name) {
            Ok(device) => {
                let exists = cpal::default_host()
                    .input_devices()
                    .map(|mut it| it.any(|d| d.name().map(|n| n == device.name).unwrap_or(false)))
                    .unwrap_or(false);
                if exists {
                    info!("Using requested microphone: '{}'", device.name);
                    return Some(Arc::new(device));
                }
                warn!("Requested mic '{}' not enumerated — falling back to system default", device.name);
            }
            Err(e) => warn!("Requested mic '{}' not available: {} — falling back to system default", name, e),
        }
    }

    match default_input_device() {
        Ok(device) => {
            info!("Using default microphone: '{}'", device.name);
            if requested_name.is_some() {
                let _ = app.emit("mic-device-switched", serde_json::json!({ "device_name": device.name }));
            }
            Some(Arc::new(device))
        }
        Err(e) => {
            warn!("No microphone available: {} — recording system audio only", e);
            let _ = app.emit("mic-unavailable", serde_json::json!({}));
            None
        }
    }
}

/// Resolve the system source: requested (parsed; `app:chrome` is the Chrome process capture,
/// which fails here when Chrome is not running) → default render endpoint → none.
pub fn resolve_system_or_default(requested_name: Option<&str>) -> Result<Option<SystemSource>, String> {
    if let Some(name) = requested_name {
        match parse_audio_device(name) {
            Ok(device) => {
                info!("Using requested system audio: '{}'", device.name);
                let source = SystemSource::for_device(Arc::new(device));
                source.check().map_err(|e| e.to_string())?;
                return Ok(Some(source));
            }
            Err(e) => warn!("Requested system audio '{}' not available: {} — using the default", name, e),
        }
    }
    match default_output_device() {
        Ok(device) => {
            info!("Using default system audio: '{}'", device.name);
            Ok(Some(SystemSource::Endpoint(Arc::new(device))))
        }
        Err(e) => {
            warn!("No system audio available: {} — recording microphone only", e);
            Ok(None)
        }
    }
}

fn default_meeting_title() -> String {
    format!("Call {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))
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
        devices.push(AudioDevice::with_label(CHROME_APP_DEVICE, DeviceType::Output, CHROME_APP_LABEL));
    }
    Ok(devices)
}

#[tauri::command]
pub async fn start_audio_level_monitoring(app: AppHandle, device_names: Vec<String>) -> Result<(), String> {
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
/// folder, then starts capture.
#[tauri::command]
pub async fn start_recording_with_devices_and_meeting(
    app: AppHandle,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<RecordingStarted, String> {
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    if IS_RECORDING.load(Ordering::SeqCst) {
        return Err("Recording already in progress".to_string());
    }
    let result = start_recording(&app, mic_device_name, system_device_name, meeting_name).await;
    // Every outcome, failures included, puts the tray back in step with the real state.
    crate::tray::update_tray_menu(&app);
    result
}

async fn start_recording(
    app: &AppHandle,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<RecordingStarted, String> {
    info!(
        "Starting recording: mic={:?}, system={:?}, meeting={:?}",
        mic_device_name, system_device_name, meeting_name
    );
    let _ = app.emit("recording-starting", serde_json::json!({}));
    crate::tray::set_tray_state(app, crate::tray::RecordingState::Starting);

    let preferences = super::recording_preferences::load_recording_preferences(app);
    let mic_name = mic_device_name.or(preferences.preferred_mic_device);
    let system_name = system_device_name.or(preferences.preferred_system_device);
    let microphone_device = resolve_mic_or_default(app, mic_name.as_deref());
    let system_source = resolve_system_or_default(system_name.as_deref())?;
    // The device test holds the same devices open.
    super::level_monitor::stop_monitoring();

    let title = meeting_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(default_meeting_title);
    let base_folder = super::recording_preferences::recordings_folder(app)?;
    let folder = super::audio_processing::create_meeting_folder(&base_folder, &title)
        .map_err(|e| format!("Failed to create meeting folder: {}", e))?;
    let folder_path = folder.to_string_lossy().to_string();
    let meeting_id = format!("meeting-{}", uuid::Uuid::new_v4());

    let pool = app.state::<AppState>().db_manager.pool().clone();
    if let Err(e) = MeetingsRepository::create(
        &pool,
        &NewMeeting { id: &meeting_id, title: &title, folder_path: &folder_path, status: meeting_status::RECORDING },
    )
    .await
    {
        remove_folder(&folder);
        return Err(format!("Failed to create meeting: {}", e));
    }

    let target = MeetingTarget { meeting_id: meeting_id.clone(), meeting_name: title, folder: folder.clone() };
    let live = live_setup(app, &meeting_id, &pool);
    let app_for_levels = app.clone();
    let levels = Box::new(move |levels: super::levels::AudioLevels| {
        let _ = app_for_levels.emit("audio-levels", levels);
    });
    let mut manager = match RecordingManager::start(target, microphone_device, system_source, live, levels).await {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to start recording: {:#}", e);
            if let Err(db_error) = MeetingsRepository::delete(&pool, &meeting_id).await {
                error!("Failed to delete meeting {} after start error: {}", meeting_id, db_error);
            }
            remove_folder(&folder);
            return Err(format!("Failed to start recording: {}", e));
        }
    };

    let app_for_error = app.clone();
    manager.set_error_callback(move |error| {
        let _ = app_for_error.emit("recording-error", serde_json::json!({ "message": error.user_message() }));
    });
    let device_event_receiver = manager.take_device_event_receiver();
    let session = manager.get_state().clone();
    *lock_manager() = Some(manager);
    IS_RECORDING.store(true, Ordering::SeqCst);
    MIC_FALLBACK_FAILED_ATTEMPTS.store(0, Ordering::SeqCst);
    if let Some(receiver) = device_event_receiver {
        spawn_device_event_processor(app.clone(), receiver, session);
    }

    let started = RecordingStarted { meeting_id, folder_path };
    let _ = app.emit("recording-started", &started);
    info!("Recording started for meeting {}", started.meeting_id);
    Ok(started)
}

/// Live transcription for a new recording, per the voice settings (`None` when disabled).
fn live_setup(app: &AppHandle, meeting_id: &str, pool: &sqlx::SqlitePool) -> Option<LiveSetup> {
    let service = app.state::<VoiceService>();
    let settings = service.settings();
    if !settings.live_enabled {
        info!("Live transcription is off");
        return None;
    }
    Some(LiveSetup {
        config: LiveConfig::from_settings(meeting_id, &settings),
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

/// Stop the recording: emit `recording-stopping`, stop capture, drain the saver, write
/// `audio.mp4`, mark the meeting `stopped`, emit `recording-stopped`, then start the final
/// transcript. The recording is over even when the audio could not be saved: that case
/// resolves with `audio_path: null` and is reported once, through `recording-error`.
#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Result<RecordingStopped, String> {
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    let Some(mut manager) = lock_manager().take() else {
        return Err("No recording in progress".to_string());
    };
    let stopping = StoppingGuard::new();
    info!("Stopping recording");
    let _ = app.emit("recording-stopping", serde_json::json!({ "meeting_id": manager.target().meeting_id }));
    crate::tray::set_tray_state(&app, crate::tray::RecordingState::Stopping);

    // Audio finalized and live sessions finished (both inside stop_and_save) before the
    // meeting is marked stopped and handed to the final transcript.
    let stopped = manager.stop_and_save().await;
    let target = manager.target().clone();
    drop(manager);
    IS_RECORDING.store(false, Ordering::SeqCst);

    let pool = app.state::<AppState>().db_manager.pool().clone();
    if let Some(live) = &stopped.live {
        let ids = serde_json::to_string(&live.session_ids).unwrap_or_else(|_| "[]".to_string());
        if let Err(e) = MeetingsRepository::set_live_info(&pool, &target.meeting_id, &live.model, live.mode, &ids).await {
            error!("Failed to store live session info for {}: {}", target.meeting_id, e);
        }
    }
    if let Err(e) = MeetingsRepository::mark_stopped(&pool, &target.meeting_id, stopped.duration_seconds).await {
        error!("Failed to mark meeting {} stopped: {}", target.meeting_id, e);
    }

    // Register automatic work before the UI opens the completed meeting.
    if stopped.audio_path.is_ok() {
        crate::final_transcript::on_recording_stopped(&app, &target.meeting_id).await;
    }
    let folder_path = target.folder.to_string_lossy().to_string();
    drop(stopping);
    let _ = app.emit(
        "recording-stopped",
        serde_json::json!({
            "meeting_id": target.meeting_id,
            "folder_path": folder_path,
            "meeting_name": target.meeting_name,
        }),
    );
    crate::tray::update_tray_menu(&app);

    let audio_path = match stopped.audio_path {
        Ok(audio_path) => {
            info!("Recording {} saved to {}", target.meeting_id, audio_path.display());
            Some(audio_path.to_string_lossy().to_string())
        }
        Err(e) => {
            let message = format!("Recording stopped but the audio could not be saved: {:#}", e);
            error!("{}", message);
            let _ = app.emit("recording-error", serde_json::json!({ "message": message }));
            None
        }
    };
    Ok(RecordingStopped { meeting_id: target.meeting_id, folder_path, audio_path })
}

/// The stop tail is running (capture ended, audio and live transcript being finished).
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
pub async fn is_recording() -> bool {
    is_recording_active()
}

#[tauri::command]
pub async fn pause_recording(app: AppHandle) -> Result<(), String> {
    if !is_recording_active() {
        return Err("No recording is currently active".to_string());
    }
    {
        let guard = lock_manager();
        let manager = guard.as_ref().ok_or_else(|| "No recording manager found".to_string())?;
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
        let manager = guard.as_ref().ok_or_else(|| "No recording manager found".to_string())?;
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
            "current_pause_duration": manager.get_current_pause_duration()
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
    lock_manager().as_ref().map(|m| m.live_lines()).unwrap_or_default()
}

#[tauri::command]
pub async fn get_recording_meeting_name() -> Option<String> {
    lock_manager().as_ref().map(|m| m.target().meeting_name.clone())
}

// ============================================================================
// MIC HOT-SWAP (disconnect recovery)
// ============================================================================

// Guard against concurrent mic hot-swap tasks. Only used by the disconnect
// fallback path (trigger_mic_fallback_to_default) — the "chase the new
// default" auto-swap has been removed.
static MIC_SWAP_IN_PROGRESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// Bounded retry budget for the disconnect fallback (P1 #2). Counts COMPLETED
// failed attempts; MIC_SWAP_IN_PROGRESS still prevents overlapping swaps.
static MIC_FALLBACK_FAILED_ATTEMPTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const MAX_MIC_FALLBACK_ATTEMPTS: u32 = 3;

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
            let _ = app.emit("mic-device-switched", serde_json::json!({
                "device_name": new_device_name
            }));
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
                    let _ = app.emit("mic-device-switched", serde_json::json!({
                        "device_name": new_device_name
                    }));
                    Ok(())
                }
                Err(e) => {
                    error!("[HOT_SWAP] Mic swap failed after retry: {}", e);
                    if session_live(session) {
                        let _ = app.emit("mic-swap-failed", serde_json::json!({
                            "error": e,
                            "device_name": new_device_name
                        }));
                    }
                    Err(e)
                }
            }
        }
    }
}

/// Phased mic swap — lock is never held during async I/O.
async fn do_mic_swap(device_name: &str, session: &Arc<super::RecordingState>) -> Result<(), String> {
    // Phase 1: Lock briefly — verify identity, take old stream OUT (no teardown under lock)
    let old_mic = {
        let mut guard = lock_manager();
        let manager = guard.as_mut().ok_or_else(|| "Recording manager not available".to_string())?;
        if !manager.is_recording() {
            return Err("Recording stopped — aborting mic hot-swap".to_string());
        }
        if !Arc::ptr_eq(manager.get_state(), session) {
            return Err("Session changed before hot-swap — aborting".to_string());
        }
        manager.take_mic_stream_for_swap()
    }; // lock released

    // Tear down the dead mic OUTSIDE the lock — cpal stop()/drop on a
    // disconnected BT device can stall; doing it
    // under RECORDING_MANAGER would freeze stop_recording (deep-review #2).
    // Non-fatal: the replacement stream is created next regardless, so a
    // teardown error/stall on the already-dead device must not abort the swap.
    if let Some(s) = old_mic {
        if let Err(e) = s.stop() {
            warn!("[HOT_SWAP] Failed to stop old mic stream (proceeding): {}", e);
        }
    }

    // Phase 2: Async I/O WITHOUT lock — may be slow, that's OK
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Build the AudioDevice directly from the name — the caller
    // (trigger_mic_fallback_to_default) already resolved it via
    // default_input_device(). Skipping list_audio_devices() here avoids a
    // full cpal enumeration on the exact BT-transition hot path where it's
    // known to hang 100+ s (see H2 in PR-175 review). The real device
    // validation happens inside AudioStream::create → get_device_and_config
    // which does a targeted host.input_devices() lookup by name.
    let device_arc = std::sync::Arc::new(AudioDevice::new(
        device_name.to_string(),
        DeviceType::Input,
    ));

    info!("[HOT_SWAP] Creating new mic stream for '{}' (lock released)", device_name);
    let new_stream = super::stream::AudioStream::create(
        device_arc.clone(),
        session.clone(),
        super::recording_state::DeviceType::Microphone,
    ).await.map_err(|e| format!("Failed to create mic stream: {}", e))?;

    // Resolve the current default output OUTSIDE the lock — a driver stall here
    // must not block stop_recording (which needs RECORDING_MANAGER). (P1 #1)
    let system_name = default_output_device().ok().map(|d| d.name);

    // Phase 3: Lock briefly — install ONLY if still the same session
    {
        let mut guard = lock_manager();
        match guard.as_mut() {
            Some(manager) if Arc::ptr_eq(manager.get_state(), session) => {
                manager.set_mic_stream_after_swap(new_stream, device_arc, system_name);
                info!("[HOT_SWAP] Mic hot-swap to '{}' completed", device_name);
            }
            Some(_) => {
                return Err("Session changed during hot-swap — discarding stale mic stream".to_string());
            }
            None => {
                return Err("Recording manager gone during hot-swap".to_string());
            }
        }
    } // lock released

    Ok(())
}

/// Background processor for device monitor events during a recording session.
///
/// The ONLY mid-recording mic switch that is allowed is the fallback from a
/// dead device to the system default, triggered by the device monitor's
/// DeviceDisconnected event. Any other device event is explicitly ignored —
/// recording stays on whatever device was picked at start time until the
/// meeting ends.
///
/// Rationale: auto-swapping to a freshly-connected BT device during recording
/// has hung cpal's stream creation. Locking
/// the device at start eliminates that hang and also makes the recording
/// session predictable.
///
/// The task stops automatically when the receiver is dropped (recording
/// ends / monitor stops).
fn spawn_device_event_processor<R: Runtime>(
    app: AppHandle<R>,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<DeviceEvent>,
    session: Arc<super::RecordingState>,
) {
    tokio::spawn(async move {
        info!("[DEVICE_EVENTS] Background event processor started");

        while let Some(event) = receiver.recv().await {
            // Skip if recording has stopped
            if !recording_live() {
                info!("[DEVICE_EVENTS] Recording stopped — ignoring event: {:?}", event);
                continue;
            }

            match event {
                DeviceEvent::DeviceDisconnected { ref device_name, ref device_type } => {
                    info!("[DEVICE_EVENTS] Device disconnected: '{}' ({:?})", device_name, device_type);
                    // The only automatic mid-recording mic change allowed:
                    // when the active microphone dies, fall back to the
                    // system default input. Triggered after the device
                    // monitor's polling threshold fires.
                    if matches!(device_type, DeviceMonitorType::Microphone) {
                        let name = device_name.clone();
                        let app_clone = app.clone();
                        let session = session.clone();
                        tokio::spawn(async move {
                            trigger_mic_fallback_to_default(app_clone, name, session).await;
                        });
                    }
                }
                DeviceEvent::DeviceReconnected { ref device_name, ref device_type } => {
                    // Per product decision: once we have fallen back to the
                    // built-in mic we stay there for the rest of the meeting.
                    // This is intentional — just log and do nothing.
                    info!("[DEVICE_EVENTS] Device reconnected: '{}' ({:?}) — staying on current mic (fallback is sticky)", device_name, device_type);
                }
                DeviceEvent::DeviceListChanged => {
                    debug!("[DEVICE_EVENTS] Device list changed");
                }
            }
        }
        info!("[DEVICE_EVENTS] Background event processor stopped (channel closed)");
    });
}

/// Disconnect fallback: swap the active mic to the system default input
/// device. Triggered from the background device event processor after the
/// device monitor's polling threshold (3 × 2s) fires `DeviceDisconnected`
/// for the active microphone.
///
/// `disconnected_name` is the device that just died. We keep it to detect
/// the edge case where the OS hasn't yet updated the system default input
/// away from the dead device — we wait and retry in that case rather than
/// swapping back to the same broken device.
///
/// This function takes the MIC_SWAP_IN_PROGRESS guard itself; the caller
/// must NOT already hold it. If a swap is somehow already running this
/// returns immediately.
async fn trigger_mic_fallback_to_default<R: Runtime>(
    app: AppHandle<R>,
    disconnected_name: String,
    session: Arc<super::RecordingState>,
) {
    if !session_live(&session) {
        info!(
            "[MIC_FALLBACK] Not recording — skipping fallback for '{}'",
            disconnected_name
        );
        return;
    }

    if MIC_FALLBACK_FAILED_ATTEMPTS.load(Ordering::SeqCst) >= MAX_MIC_FALLBACK_ATTEMPTS {
        warn!(
            "[MIC_FALLBACK] {} failed attempts reached — giving up on '{}' (terminal event already announced)",
            MAX_MIC_FALLBACK_ATTEMPTS, disconnected_name
        );
        return;
    }

    if MIC_SWAP_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        info!(
            "[MIC_FALLBACK] Swap already in progress — skipping fallback for '{}'",
            disconnected_name
        );
        return;
    }

    // Guard that clears MIC_SWAP_IN_PROGRESS on any return path below so a
    // panic or early return can't leave the flag stuck.
    struct SwapGuard;
    impl Drop for SwapGuard {
        fn drop(&mut self) {
            MIC_SWAP_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
    }
    let _guard = SwapGuard;

    info!(
        "[MIC_FALLBACK] Starting fallback from disconnected device '{}'",
        disconnected_name
    );

    // Let the OS finish swapping the system default input away from the dead
    // device. 150ms is enough in practice for the built-in mic to become the
    // default when an explicitly-selected BT device disconnects.
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // A Stop-A/Start-B during the sleep swapped our session out. Bail silently —
    // emitting or spending the recovery budget here would fire against B with
    // A's device. Covers the default_input_device() error branch below.
    if !session_live(&session) {
        info!("[MIC_FALLBACK] Session no longer live after wait — aborting fallback for '{}'", disconnected_name);
        return;
    }

    // Query the current system default input. If it still reports the
    // disconnected device, back off once more and re-query — this handles
    // the edge case where the OS hasn't propagated the change yet.
    let fallback_name = match default_input_device() {
        Ok(dev) => dev.name,
        Err(e) => {
            error!("[MIC_FALLBACK] Failed to query default input device: {}", e);
            let _ = app.emit(
                "mic-swap-failed",
                serde_json::json!({
                    "error": format!("Failed to query default input: {}", e),
                    "device_name": disconnected_name,
                }),
            );
            let n = MIC_FALLBACK_FAILED_ATTEMPTS.fetch_add(1, Ordering::SeqCst) + 1;
            if n == MAX_MIC_FALLBACK_ATTEMPTS {
                let _ = app.emit(
                    "mic-recovery-exhausted",
                    serde_json::json!({ "device_name": disconnected_name }),
                );
            }
            return;
        }
    };

    let fallback_name = if fallback_name == disconnected_name {
        warn!(
            "[MIC_FALLBACK] Default input still reports disconnected device '{}' — retrying after 300ms",
            disconnected_name
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        if !session_live(&session) {
            info!("[MIC_FALLBACK] Session no longer live after retry wait — aborting fallback for '{}'", disconnected_name);
            return;
        }
        match default_input_device() {
            Ok(dev) if dev.name != disconnected_name => dev.name,
            Ok(dev) => {
                error!(
                    "[MIC_FALLBACK] Default input still '{}' after retry — aborting fallback",
                    dev.name
                );
                let _ = app.emit(
                    "mic-swap-failed",
                    serde_json::json!({
                        "error": "System default input still reports disconnected device after retry",
                        "device_name": disconnected_name,
                    }),
                );
                let n = MIC_FALLBACK_FAILED_ATTEMPTS.fetch_add(1, Ordering::SeqCst) + 1;
                if n == MAX_MIC_FALLBACK_ATTEMPTS {
                    let _ = app.emit(
                        "mic-recovery-exhausted",
                        serde_json::json!({ "device_name": disconnected_name }),
                    );
                }
                return;
            }
            Err(e) => {
                error!("[MIC_FALLBACK] Failed to re-query default input device: {}", e);
                let _ = app.emit(
                    "mic-swap-failed",
                    serde_json::json!({
                        "error": format!("Failed to re-query default input: {}", e),
                        "device_name": disconnected_name,
                    }),
                );
                let n = MIC_FALLBACK_FAILED_ATTEMPTS.fetch_add(1, Ordering::SeqCst) + 1;
                if n == MAX_MIC_FALLBACK_ATTEMPTS {
                    let _ = app.emit(
                        "mic-recovery-exhausted",
                        serde_json::json!({ "device_name": disconnected_name }),
                    );
                }
                return;
            }
        }
    } else {
        fallback_name
    };

    info!(
        "[MIC_FALLBACK] Falling back '{}' → '{}'",
        disconnected_name, fallback_name
    );

    // Stop may have started during the sleeps above — bail before touching
    // the (possibly already taken) manager.
    if !session_live(&session) {
        info!("[MIC_FALLBACK] Recording stopping — aborting fallback for '{}'", disconnected_name);
        return;
    }

    // perform_mic_hot_swap_task performs its own retry-once logic on failure
    // and emits the mic-device-switched / mic-swap-failed events, so we can
    // just delegate here. It does NOT touch MIC_SWAP_IN_PROGRESS internally.
    match perform_mic_hot_swap_task(fallback_name.clone(), &session, app.clone()).await {
        Ok(()) => {
            info!(
                "[MIC_FALLBACK] Fallback complete: now recording via '{}'",
                fallback_name
            );
            MIC_FALLBACK_FAILED_ATTEMPTS.store(0, Ordering::SeqCst);
        }
        Err(e) => {
            error!("[MIC_FALLBACK] Fallback swap failed: {}", e);
            if !session_live(&session) {
                return;
            }
            let n = MIC_FALLBACK_FAILED_ATTEMPTS.fetch_add(1, Ordering::SeqCst) + 1;
            if n == MAX_MIC_FALLBACK_ATTEMPTS {
                let _ = app.emit(
                    "mic-recovery-exhausted",
                    serde_json::json!({ "device_name": disconnected_name }),
                );
            }
        }
    }
}
