//! Signed updates from the published release feed. Recording owns the restart boundary.
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{ipc::Channel, Manager};
use tauri_plugin_updater::UpdaterExt;

static BUSY: AtomicBool = AtomicBool::new(false);
struct UpdateGuard;
impl Drop for UpdateGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Serialize)]
pub struct UpdateInfo {
    version: String,
    notes: String,
}
#[derive(Clone, Serialize)]
pub struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
    installing: bool,
}

#[tauri::command]
pub async fn check_app_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("Update check failed: {e}"))?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body.unwrap_or_default(),
    }))
}

#[tauri::command]
pub async fn install_app_update(
    app: tauri::AppHandle,
    version: String,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), String> {
    BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "An update is already in progress".to_string())?;
    let _guard = UpdateGuard;
    if crate::audio::recording_commands::is_recording_active() {
        return Err("Finish recording before installing the update.".into());
    }
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("This installation is current. Check for updates again later.")?;
    if update.version != version {
        return Err("The release changed. Check for updates again.".into());
    }
    let mut downloaded = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = on_progress.send(UpdateProgress {
                    downloaded,
                    total,
                    installing: false,
                });
            },
            || {},
        )
        .await
        .map_err(|e| format!("Update download or signature verification failed: {e}"))?;
    // Serialize with start/stop, including the final recording save.
    let _lifecycle = crate::audio::recording_commands::LIFECYCLE_LOCK
        .lock()
        .await;
    if crate::audio::recording_commands::is_recording_active() {
        return Err(
            "Recording started during download. Finish recording and try the update again.".into(),
        );
    }
    let pool = app
        .state::<crate::state::AppState>()
        .db_manager
        .pool()
        .clone();
    let jobs: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM meetings WHERE final_status IN ('queued','uploading','transcribing')) + (SELECT COUNT(*) FROM summary_processes WHERE LOWER(status) = 'pending')")
        .fetch_one(&pool).await.map_err(|e| e.to_string())?;
    if jobs > 0 || crate::audio::import::is_import_active() || crate::publication::is_busy() {
        return Err(
            "Wait for audio processing, summaries and uploads to finish, then install the update."
                .into(),
        );
    }
    sqlx::query("PRAGMA wal_checkpoint(FULL)")
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let _ = on_progress.send(UpdateProgress {
        downloaded,
        total: Some(downloaded),
        installing: true,
    });
    update
        .install(bytes)
        .map_err(|e| format!("Installation failed: {e}"))?;
    app.restart();
}
