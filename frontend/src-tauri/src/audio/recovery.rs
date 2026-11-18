//! Startup recovery of recordings whose audio is still in the journal: interrupted by a
//! crash or power loss, or stopped while encoding was unavailable.

use super::constants::find_meeting_audio;
use super::incremental_saver::{encode_journal, has_journal};
use super::pipeline::PIPELINE_SAMPLE_RATE;
use crate::database::repositories::meeting::MeetingsRepository;
use log::{error, info, warn};
use std::collections::HashSet;
use std::path::PathBuf;

/// Mark meetings still `recording` as `interrupted`, then encode every meeting's
/// remaining audio journal into `audio.mp4` and hand it to the final transcript. An
/// interrupted meeting whose audio was already encoded (the app died between the encode
/// and the status update) goes straight to the final transcript.
pub async fn recover_interrupted_meetings(app: &tauri::AppHandle, pool: &sqlx::SqlitePool) {
    let interrupted: HashSet<String> = match MeetingsRepository::interrupt_unfinished(pool).await {
        Ok(meetings) => meetings.into_iter().map(|meeting| meeting.id).collect(),
        Err(e) => {
            error!("Failed to look up interrupted meetings: {}", e);
            return;
        }
    };
    let meetings = match MeetingsRepository::list(pool).await {
        Ok(meetings) => meetings,
        Err(e) => {
            error!("Failed to list meetings for recovery: {}", e);
            return;
        }
    };
    for meeting in meetings {
        let was_interrupted = interrupted.contains(&meeting.id);
        let Some(folder) = meeting.folder_path.as_deref().map(PathBuf::from) else {
            if was_interrupted {
                warn!("Interrupted meeting {} has no folder", meeting.id);
            }
            continue;
        };
        if !has_journal(&folder) {
            if was_interrupted {
                match find_meeting_audio(&folder) {
                    Some(path) => {
                        info!(
                            "Interrupted meeting {} already has {}",
                            meeting.id,
                            path.display()
                        );
                        crate::final_transcript::on_meeting_recovered(app, &meeting.id);
                    }
                    None => warn!("Interrupted meeting {} has no audio", meeting.id),
                }
            }
            continue;
        }
        info!(
            "Recovering audio of meeting {} ({})",
            meeting.id, meeting.title
        );
        let encode =
            tokio::task::spawn_blocking(move || encode_journal(&folder, PIPELINE_SAMPLE_RATE))
                .await;
        match encode {
            Ok(Ok(path)) => {
                info!("Recovered audio for {}: {}", meeting.id, path.display());
                crate::final_transcript::on_meeting_recovered(app, &meeting.id);
            }
            Ok(Err(e)) => error!("Failed to recover audio for {}: {:#}", meeting.id, e),
            Err(e) => error!("Recovery task for {} failed: {}", meeting.id, e),
        }
    }
}
