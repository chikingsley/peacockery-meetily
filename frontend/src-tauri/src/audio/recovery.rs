//! Startup recovery of recordings interrupted by a crash or power loss.

use super::constants::find_meeting_audio;
use super::incremental_saver::{checkpoint_files, merge_checkpoints};
use crate::database::repositories::meeting::MeetingsRepository;
use log::{error, info, warn};
use std::path::PathBuf;

/// Mark meetings still `recording` as `interrupted`, merge their audio checkpoints into
/// `audio.mp4`, then hand each one to the final transcript. A meeting whose audio was
/// already merged (the app died between the merge and the status update) goes straight to
/// the final transcript.
pub async fn recover_interrupted_meetings(app: &tauri::AppHandle, pool: &sqlx::SqlitePool) {
    let interrupted = match MeetingsRepository::interrupt_unfinished(pool).await {
        Ok(meetings) => meetings,
        Err(e) => {
            error!("Failed to look up interrupted meetings: {}", e);
            return;
        }
    };
    for meeting in interrupted {
        info!("Recovering interrupted meeting {} ({})", meeting.id, meeting.title);
        let Some(folder) = meeting.folder_path.as_deref().map(PathBuf::from) else {
            warn!("Interrupted meeting {} has no folder", meeting.id);
            continue;
        };
        if checkpoint_files(&folder).is_empty() {
            match find_meeting_audio(&folder) {
                Some(path) => {
                    info!("Interrupted meeting {} already has {}", meeting.id, path.display());
                    crate::final_transcript::on_meeting_recovered(app, &meeting.id);
                }
                None => warn!("Interrupted meeting {} has no audio", meeting.id),
            }
            continue;
        }
        let merge = tokio::task::spawn_blocking(move || merge_checkpoints(&folder)).await;
        match merge {
            Ok(Ok(path)) => {
                info!("Recovered audio for {}: {}", meeting.id, path.display());
                crate::final_transcript::on_meeting_recovered(app, &meeting.id);
            }
            Ok(Err(e)) => error!("Failed to recover audio for {}: {:#}", meeting.id, e),
            Err(e) => error!("Recovery task for {} failed: {}", meeting.id, e),
        }
    }
}
