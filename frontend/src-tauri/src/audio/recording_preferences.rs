//! Recording preferences (store file `recording_preferences.json`). Audio is always
//! saved; there is no auto-save toggle and no format choice (AAC in `audio.mp4`).

use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "recording_preferences.json";
const STORE_KEY: &str = "preferences";
const RECORDINGS_FOLDER_NAME: &str = "Peacockery Meetily";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RecordingPreferences {
    pub save_folder: PathBuf,
    #[serde(default)]
    pub preferred_mic_device: Option<String>,
    #[serde(default)]
    pub preferred_system_device: Option<String>,
}

/// `<Music>\Peacockery Meetily`, or `<app data>\recordings` when the Music known folder
/// is unavailable. Never Documents (OneDrive-redirected on CICA).
pub fn default_recordings_folder<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    if let Some(music_dir) = dirs::audio_dir() {
        return music_dir.join(RECORDINGS_FOLDER_NAME);
    }
    match app.path().app_data_dir() {
        Ok(app_data) => app_data.join("recordings"),
        Err(e) => {
            warn!("No Music folder and no app data dir ({}); using the working directory", e);
            PathBuf::from(RECORDINGS_FOLDER_NAME)
        }
    }
}

fn default_preferences<R: Runtime>(app: &AppHandle<R>) -> RecordingPreferences {
    RecordingPreferences {
        save_folder: default_recordings_folder(app),
        preferred_mic_device: None,
        preferred_system_device: None,
    }
}

/// Stored preferences, or defaults when the store is missing or unreadable.
pub fn load_recording_preferences<R: Runtime>(app: &AppHandle<R>) -> RecordingPreferences {
    let stored = match app.store(STORE_FILE) {
        Ok(store) => store.get(STORE_KEY),
        Err(e) => {
            warn!("Failed to open {}: {}", STORE_FILE, e);
            None
        }
    };
    match stored.map(serde_json::from_value::<RecordingPreferences>) {
        Some(Ok(prefs)) => prefs,
        Some(Err(e)) => {
            warn!("Ignoring unreadable recording preferences: {}", e);
            default_preferences(app)
        }
        None => default_preferences(app),
    }
}

/// Let the webview's asset protocol read the recordings folder, wherever the user put it
/// (the static scope in tauri.conf.json only covers the default locations).
pub fn allow_recordings_folder<R: Runtime>(app: &AppHandle<R>) {
    let folder = load_recording_preferences(app).save_folder;
    if let Err(e) = app.asset_protocol_scope().allow_directory(&folder, true) {
        warn!("Failed to allow {} for audio playback: {}", folder.display(), e);
    }
}

/// The folder new meetings are created in (created if missing).
pub fn recordings_folder<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let folder = load_recording_preferences(app).save_folder;
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Failed to create recordings folder {}: {}", folder.display(), e))?;
    Ok(folder)
}

#[tauri::command]
pub async fn get_recording_preferences<R: Runtime>(app: AppHandle<R>) -> Result<RecordingPreferences, String> {
    Ok(load_recording_preferences(&app))
}

#[tauri::command]
pub async fn set_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: RecordingPreferences,
) -> Result<(), String> {
    std::fs::create_dir_all(&preferences.save_folder).map_err(|e| {
        format!("Failed to create recordings folder {}: {}", preferences.save_folder.display(), e)
    })?;
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("Failed to open {}: {}", STORE_FILE, e))?;
    let value = serde_json::to_value(&preferences).map_err(|e| e.to_string())?;
    store.set(STORE_KEY, value);
    store
        .save()
        .map_err(|e| format!("Failed to save recording preferences: {}", e))?;
    allow_recordings_folder(&app);
    info!(
        "Saved recording preferences: save_folder={:?}, mic={:?}, system={:?}",
        preferences.save_folder, preferences.preferred_mic_device, preferences.preferred_system_device
    );
    Ok(())
}

#[tauri::command]
pub async fn get_default_recordings_folder_path<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    Ok(default_recordings_folder(&app).to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_recordings_folder<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let folder = recordings_folder(&app)?;
    crate::utils::open_folder(&folder)
}
