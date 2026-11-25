//! Startup recovery of recordings whose audio is still in the journal: interrupted by a
//! crash or power loss, stopped while encoding was unavailable, or quit before the
//! background encode finished.

use super::audio_save;
use super::constants::find_meeting_audio;
use super::journal::{has_journal, CHECKPOINTS_DIR};
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
        // Older builds only indexed the calendar link after successful model review.
        if let Some(key) = crate::recording_context::RecordingContext::load(&folder)
            .and_then(|context| {
                context.brief_key.or_else(|| {
                    context
                        .calendar_event
                        .map(|event| format!("event:{}", event.id))
                })
            })
            .filter(|key| key.starts_with("event:"))
        {
            if let Err(error) = sqlx::query("INSERT INTO meeting_reviews(meeting_id,brief_key) VALUES(?,?) ON CONFLICT(meeting_id) DO UPDATE SET brief_key=excluded.brief_key WHERE brief_key IS NULL")
                .bind(&meeting.id).bind(key).execute(pool).await {
                warn!("Could not index saved calendar link: {error}");
            }
        }
        let audio = find_meeting_audio(&folder);
        if has_journal(&folder) && audio.is_some() {
            // `audio.*` is renamed in only once complete, so this encode finished and only
            // the journal removal did not (crash right after the rename, or a locked file).
            // The audio is not encoded again and its automatic work does not run twice.
            let checkpoints = folder.join(CHECKPOINTS_DIR);
            match std::fs::remove_dir_all(&checkpoints) {
                Ok(()) => info!("Removed the leftover journal of meeting {}", meeting.id),
                Err(e) => warn!("Failed to remove {}: {}", checkpoints.display(), e),
            }
            if was_interrupted {
                recovery.encoded.push(meeting.id);
            }
        } else if has_journal(&folder) {
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
            match audio {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::meeting::NewMeeting;
    use std::path::Path;

    fn journal_in(folder: &Path) {
        let checkpoints = folder.join(CHECKPOINTS_DIR);
        std::fs::create_dir_all(&checkpoints).unwrap();
        std::fs::write(checkpoints.join("audio.s16le"), [0u8; 64]).unwrap();
    }

    async fn seed(pool: &sqlx::SqlitePool, id: &str, folder: &Path, status: &str) {
        MeetingsRepository::create(
            pool,
            &NewMeeting {
                id,
                title: "Meeting",
                folder_path: &folder.to_string_lossy(),
                status,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn journals_are_routed_by_status_and_finished_encodes_are_not_repeated() {
        let pool = crate::database::test_pool().await;
        let root = tempfile::tempdir().unwrap();
        let folder = |name: &str| {
            let path = root.path().join(name);
            std::fs::create_dir_all(&path).unwrap();
            path
        };

        // Crashed while recording: the journal is encoded, then its automatic work runs.
        let crashed = folder("crashed");
        journal_in(&crashed);
        seed(&pool, "rec-crashed", &crashed, meeting_status::RECORDING).await;
        // Interrupted at an earlier start whose encode did not finish.
        let earlier = folder("earlier");
        journal_in(&earlier);
        seed(&pool, "rec-earlier", &earlier, meeting_status::INTERRUPTED).await;
        // Stopped: its automatic work ran at stop.
        let stopped = folder("stopped");
        journal_in(&stopped);
        seed(&pool, "rec-stopped", &stopped, meeting_status::STOPPED).await;
        // Encoded earlier, but the journal could not be removed.
        let leftover = folder("leftover");
        journal_in(&leftover);
        std::fs::write(leftover.join("audio.mp4"), b"mp4").unwrap();
        seed(
            &pool,
            "rec-leftover",
            &leftover,
            meeting_status::INTERRUPTED,
        )
        .await;
        // Crashed right after the encode renamed audio.mp4 in.
        let renamed = folder("renamed");
        journal_in(&renamed);
        std::fs::write(renamed.join("audio.mp4"), b"mp4").unwrap();
        seed(&pool, "rec-renamed", &renamed, meeting_status::RECORDING).await;

        let recovery = register_journals(&pool).await;
        let mut journals: Vec<(String, bool)> = recovery
            .journals
            .iter()
            .map(|(id, _, pending)| (id.clone(), *pending))
            .collect();
        journals.sort();
        assert_eq!(
            journals,
            vec![
                ("rec-crashed".to_string(), true),
                ("rec-earlier".to_string(), true),
                ("rec-stopped".to_string(), false),
            ]
        );
        assert_eq!(recovery.encoded, vec!["rec-renamed".to_string()]);
        assert!(!has_journal(&leftover));
        assert!(!has_journal(&renamed));
        for id in ["rec-crashed", "rec-earlier", "rec-stopped"] {
            audio_save::finish(id, Err("test".into()));
        }
    }
}
