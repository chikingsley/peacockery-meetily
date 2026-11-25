//! Close the selected PC's AI desktop apps during capture and restore them on stop or recovery.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

const ENABLED: &str = "meeting-focus.enabled";
const RECEIPT: &str = "meeting-focus-apps.json";
static OPERATION: Mutex<()> = Mutex::const_new(());

fn directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn meeting_focus_enabled(app: AppHandle) -> Result<bool, String> {
    Ok(directory(&app)?.join(ENABLED).is_file())
}

#[tauri::command]
pub async fn set_meeting_focus(app: AppHandle, enabled: bool) -> Result<(), String> {
    let _capture = crate::audio::recording_commands::LIFECYCLE_LOCK
        .lock()
        .await;
    if crate::audio::recording_commands::is_recording_active() {
        return Err("Change meeting focus after recording stops.".into());
    }
    let dir = directory(&app)?;
    let marker = dir.join(ENABLED);
    if enabled {
        std::fs::write(marker, []).map_err(|error| error.to_string())?;
    } else if marker.exists() {
        std::fs::remove_file(marker).map_err(|error| error.to_string())?;
    }
    drop(_capture);
    reconcile(&dir, false).await
}

async fn reconcile(dir: &Path, restore_only: bool) -> Result<(), String> {
    let _operation = OPERATION.lock().await;
    let close = !restore_only
        && crate::audio::recording_commands::is_recording_active()
        && dir.join(ENABLED).is_file();
    if !close && !dir.join(RECEIPT).is_file() {
        return Ok(());
    }
    let action = if close { "close" } else { "restore" };
    let mut command = tokio::process::Command::new("powershell.exe");
    let script: Vec<u8> = include_str!("meeting_focus.ps1")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    command
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(STANDARD.encode(script))
        .env("MEETILY_FOCUS_DIR", dir)
        .env("MEETILY_FOCUS_ACTION", action)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(65), command.output())
        .await
        .map_err(|_| format!("Desktop app {action} timed out"))?
        .map_err(|error| error.to_string())?;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        log::info!("Meeting focus: {line}");
    }
    if !output.status.success() {
        return Err(format!(
            "Desktop app {action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// A serialized worker re-reads current capture state after acquiring the lock. Rapid stop/start
/// transitions therefore converge on the newest state, including when PowerShell is still busy.
pub fn changed(app: &AppHandle) {
    let Ok(dir) = directory(app) else { return };
    #[cfg(not(windows))]
    {
        let _ = dir;
    }
    #[cfg(windows)]
    tauri::async_runtime::spawn(async move {
        for delay in [0, 2, 10] {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            match reconcile(&dir, false).await {
                Ok(()) => return,
                Err(error) => log::warn!("Meeting focus: {error}"),
            }
        }
    });
}

pub async fn restore(app: &AppHandle) -> Result<(), String> {
    reconcile(&directory(app)?, true).await
}
