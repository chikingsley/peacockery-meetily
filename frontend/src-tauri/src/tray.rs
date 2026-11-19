use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

const TRAY_ID: &str = "main-tray";

/// Longest quit waits for background audio saves; an unfinished journal is encoded at the
/// next start.
const QUIT_SAVE_LIMIT: Duration = Duration::from_secs(5 * 60);

/// Quit has started; later quit requests wait for it.
static QUITTING: AtomicBool = AtomicBool::new(false);
/// Quit has finished its work, so the exit it requests is not held back again.
static EXIT_READY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
pub enum RecordingState {
    Stopped,
    Starting,
    Recording,
    Pausing,
    Paused,
    Resuming,
    Stopping,
    /// Quit waits for background audio saves.
    SavingAudio,
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, RecordingState::Stopped)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Peacockery Meetily")
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    update_tray_menu(app);
    Ok(())
}

fn handle_menu_event(app: &AppHandle, item_id: &str) {
    match item_id {
        "start_recording" => start_recording_handler(app),
        "pause_recording" => pause_recording_handler(app),
        "resume_recording" => resume_recording_handler(app),
        "stop_recording" => stop_recording_handler(app),
        "open_window" => focus_main_window(app),
        "settings" => {
            focus_main_window(app);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval("window.location.assign('/settings')");
            }
        }
        "quit" => quit(app),
        _ => {}
    }
}

/// Quit the app. A running recording is stopped first and pending audio saves are awaited
/// (at most five minutes), so the meeting is marked stopped and its `audio.mp4` written
/// instead of being left for startup recovery.
pub fn quit(app: &AppHandle) {
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Repeats when a recording was started while quit waited.
        while crate::audio::recording_commands::is_recording_active()
            || crate::audio::audio_save::pending() > 0
        {
            if crate::audio::recording_commands::is_recording_active() {
                log::info!("Quit requested while recording: stopping the recording first");
                // "No recording in progress" here means another stop is running; that stop
                // holds the lifecycle lock and registers its save before releasing it.
                if let Err(e) = crate::audio::recording_commands::stop_recording(app.clone()).await
                {
                    log::info!("Stop before quit: {}", e);
                }
            }
            if crate::audio::audio_save::pending() > 0 {
                log::info!("Quit waits for the audio saves");
                update_tray_menu_async(&app).await;
                if !crate::audio::audio_save::wait_for_pending(QUIT_SAVE_LIMIT).await {
                    log::warn!(
                        "Quitting with audio still saving; the journal is encoded at the next start"
                    );
                    break;
                }
            }
        }
        EXIT_READY.store(true, Ordering::SeqCst);
        app.exit(0);
    });
}

/// Quit has finished stopping and saving; the app may exit.
pub fn exit_ready() -> bool {
    EXIT_READY.load(Ordering::SeqCst)
}

/// Starting needs the device picks and the key check of the main window, so the tray asks
/// the page to start. The tray shows "Starting..." only once the start command runs.
fn start_recording_handler(app: &AppHandle) {
    focus_main_window(app);
    if let Err(e) = app.emit("request-recording-toggle", serde_json::json!({})) {
        log::error!("Failed to ask the main window to start recording: {}", e);
    }
}

fn pause_recording_handler(app: &AppHandle) {
    set_tray_state(app, RecordingState::Pausing);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::pause_recording(app.clone()).await {
            log::error!("Failed to pause recording from tray: {}", e);
            update_tray_menu_async(&app).await;
        }
    });
}

fn resume_recording_handler(app: &AppHandle) {
    set_tray_state(app, RecordingState::Resuming);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::resume_recording(app.clone()).await {
            log::error!("Failed to resume recording from tray: {}", e);
            update_tray_menu_async(&app).await;
        }
    });
}

/// Stop is fully Rust-owned: `stop_recording` closes the audio journal, updates the meeting,
/// emits `recording-stopped` and encodes the audio in the background.
fn stop_recording_handler(app: &AppHandle) {
    set_tray_state(app, RecordingState::Stopping);
    focus_main_window(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::stop_recording(app.clone()).await {
            log::error!("Failed to stop recording from tray: {}", e);
            update_tray_menu_async(&app).await;
        }
    });
}

/// Refresh the tray menu from the current recording state.
pub fn update_tray_menu(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        update_tray_menu_async(&app).await;
    });
}

pub fn set_tray_state(app: &AppHandle, state: RecordingState) {
    match (build_menu(app, state), app.tray_by_id(TRAY_ID)) {
        (Ok(menu), Some(tray)) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                log::warn!("Tray: failed to set menu: {}", e);
            }
        }
        (Err(e), _) => log::error!("Tray: failed to build menu: {}", e),
        (_, None) => log::warn!("Tray: no tray icon '{}'", TRAY_ID),
    }
}

fn current_recording_state() -> RecordingState {
    if crate::audio::recording_commands::is_recording_stopping() {
        RecordingState::Stopping
    } else if QUITTING.load(Ordering::SeqCst) && crate::audio::audio_save::pending() > 0 {
        RecordingState::SavingAudio
    } else if !crate::audio::recording_commands::is_recording_active() {
        RecordingState::Stopped
    } else if crate::audio::recording_commands::is_recording_paused() {
        RecordingState::Paused
    } else {
        RecordingState::Recording
    }
}

pub async fn update_tray_menu_async(app: &AppHandle) {
    set_tray_state(app, current_recording_state());
}

fn build_menu(app: &AppHandle, state: RecordingState) -> tauri::Result<Menu<tauri::Wry>> {
    let mut builder = MenuBuilder::new(app);
    let disabled = |label: &str| MenuItemBuilder::new(label).enabled(false).build(app);
    let stop = || MenuItemBuilder::with_id("stop_recording", "Stop Recording").build(app);

    builder = match state {
        RecordingState::Stopped => {
            builder.item(&MenuItemBuilder::with_id("start_recording", "New recording…").build(app)?)
        }
        RecordingState::Starting => builder.item(&disabled("Starting Recording...")?),
        RecordingState::Recording => builder
            .item(&MenuItemBuilder::with_id("pause_recording", "Pause Recording").build(app)?)
            .item(&stop()?),
        RecordingState::Pausing => builder.item(&disabled("Pausing...")?).item(&stop()?),
        RecordingState::Paused => builder
            .item(&MenuItemBuilder::with_id("resume_recording", "Resume Recording").build(app)?)
            .item(&stop()?),
        RecordingState::Resuming => builder.item(&disabled("Resuming...")?).item(&stop()?),
        RecordingState::Stopping => builder.item(&disabled("Stopping...")?),
        RecordingState::SavingAudio => builder.item(&disabled("Saving audio…")?),
    };

    builder
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("open_window", "Open Main Window").build(app)?)
        .item(&MenuItemBuilder::with_id("settings", "Settings").build(app)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?)
        .build()
}

pub(crate) fn focus_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        log::warn!("Could not find main window");
        return;
    };
    if let Err(e) = window.unminimize() {
        log::error!("Failed to unminimize main window: {}", e);
    }
    if let Err(e) = window.show() {
        log::error!("Failed to show main window: {}", e);
    }
    if let Err(e) = window.set_focus() {
        log::error!("Failed to focus main window: {}", e);
    }
}
