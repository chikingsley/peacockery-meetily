//! Startup recovery of recordings whose audio is still in the journal: interrupted by a
//! crash or power loss, stopped while encoding was unavailable, or quit before the
//! background encode finished.

use super::audio_save;
use super::constants::find_meeting_audio;
use super::journal::has_journal;
use crate::database::models::meeting_status;
use crate::database::repositories::meeting::MeetingsRepository;
use log::{error, info, warn};
use std::collections::HashSet;
use std::path::PathBuf;

/// Meetings found at startup, between [`register_journals`] and [`resume`].
#[derive(Default)]
pub struct Recovery {
    /// Interrupted meetings whose audio was already encoded before the crash.
    encoded: Vec<String>,
    /// Meetings with a journal, registered as saving: (id, folder, automatic work pending).
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
            // An interrupted meeting with a journal never ran its automatic work (that runs
            // after its audio is saved), whether it was interrupted now or at an earlier start
            // whose encode did not finish. A stopped one ran it at stop.
            let automatic_work_pending = meeting.status == meeting_status::INTERRUPTED;
            recovery
                .journals
                .push((meeting.id, folder, automatic_work_pending));
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
/// Meetings interrupted by a crash start their automatic work after their audio is saved.
/// A meeting that was already stopped ran its automatic work at stop: a queued final
/// resumes through `resume_unfinished`, and a needed final that never ran or failed
/// waiting for the audio starts once the audio is saved.
pub fn resume(app: &tauri::AppHandle, recovery: Recovery) {
    for meeting_id in recovery.encoded {
        crate::final_transcript::on_meeting_recovered(app, &meeting_id);
    }
    for (meeting_id, folder, automatic_work_pending) in recovery.journals {
        let app = app.clone();
        let id = meeting_id.clone();
        audio_save::queue(&meeting_id, folder, move |result| {
            audio_save::report(&app, &id, result);
            if result.is_ok() {
                if automatic_work_pending {
                    crate::final_transcript::on_meeting_recovered(&app, &id);
                } else {
                    crate::final_transcript::on_stopped_audio_saved(&app, &id);
                }
            }
        });
    }
}
