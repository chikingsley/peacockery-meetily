//! Startup recovery of recordings whose audio is still in the journal: interrupted by a
//! crash or power loss, stopped while encoding was unavailable, or quit before the
//! background encode finished.

use super::audio_save;
use super::constants::find_meeting_audio;
use super::journal::has_journal;
use crate::database::repositories::meeting::MeetingsRepository;
use log::{error, info, warn};
use std::collections::HashSet;
use std::path::PathBuf;

/// Meetings found at startup, between [`register_journals`] and [`resume`].
#[derive(Default)]
pub struct Recovery {
    /// Interrupted meetings whose audio was already encoded before the crash.
    encoded: Vec<String>,
    /// Meetings with a journal, registered as saving: (id, folder, interrupted now).
    journals: Vec<(String, PathBuf, bool)>,
}

/// Mark meetings still `recording` as `interrupted` and register every meeting that still
/// has an audio journal as saving. Runs before `final_transcript::resume_unfinished`, so a
/// resumed final transcript waits for the encode.
pub async fn register_journals(pool: &sqlx::SqlitePool) -> Recovery {
    let mut recovery = Recovery::default();
    let interrupted: HashSet<String> = match MeetingsRepository::interrupt_unfinished(pool).await {
        Ok(meetings) => meetings.into_iter().map(|meeting| meeting.id).collect(),
        Err(e) => {
            error!("Failed to look up interrupted meetings: {}", e);
            return recovery;
        }
    };
    let meetings = match MeetingsRepository::list(pool).await {
        Ok(meetings) => meetings,
        Err(e) => {
            error!("Failed to list meetings for recovery: {}", e);
            return recovery;
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
        if has_journal(&folder) {
            info!(
                "Recovering audio of meeting {} ({})",
                meeting.id, meeting.title
            );
            audio_save::register(&meeting.id);
            recovery
                .journals
                .push((meeting.id, folder, was_interrupted));
        } else if was_interrupted {
            match find_meeting_audio(&folder) {
                Some(path) => {
                    info!(
                        "Interrupted meeting {} already has {}",
                        meeting.id,
                        path.display()
                    );
                    recovery.encoded.push(meeting.id);
                }
                None => warn!("Interrupted meeting {} has no audio", meeting.id),
            }
        }
    }
    recovery
}

/// Start the automatic work of interrupted meetings and queue the registered encodes.
/// Only meetings interrupted by a crash start their automatic work, after their audio is
/// saved; a meeting that was already stopped had its work started at stop, and its queued
/// final transcript resumes through `resume_unfinished`.
pub fn resume(app: &tauri::AppHandle, recovery: Recovery) {
    for meeting_id in recovery.encoded {
        crate::final_transcript::on_meeting_recovered(app, &meeting_id);
    }
    for (meeting_id, folder, was_interrupted) in recovery.journals {
        let app = app.clone();
        let id = meeting_id.clone();
        audio_save::queue(&meeting_id, folder, move |result| {
            audio_save::report(&app, &id, result);
            if result.is_ok() && was_interrupted {
                crate::final_transcript::on_meeting_recovered(&app, &id);
            }
        });
    }
}
