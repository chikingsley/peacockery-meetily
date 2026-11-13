use crate::database::models::{meeting_status, Meeting};
use crate::database::now_rfc3339;
use sqlx::SqlitePool;

/// Columns needed to create a meeting row.
#[derive(Debug, Clone)]
pub struct NewMeeting<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub folder_path: &'a str,
    pub status: &'a str,
}

/// `meetings.final_*` values.
#[derive(Debug, Clone, Copy)]
pub struct FinalState<'a> {
    pub status: &'a str,
    pub model: &'a str,
    pub job_id: Option<&'a str>,
    pub error: Option<&'a str>,
}

pub struct MeetingsRepository;

impl MeetingsRepository {
    pub async fn create(pool: &SqlitePool, meeting: &NewMeeting<'_>) -> Result<(), sqlx::Error> {
        let now = now_rfc3339();
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path, status)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(meeting.id)
        .bind(meeting.title)
        .bind(&now)
        .bind(&now)
        .bind(meeting.folder_path)
        .bind(meeting.status)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// All meetings, newest first.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<Meeting>, sqlx::Error> {
        sqlx::query_as::<_, Meeting>("SELECT * FROM meetings ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    pub async fn get(pool: &SqlitePool, meeting_id: &str) -> Result<Option<Meeting>, sqlx::Error> {
        sqlx::query_as::<_, Meeting>("SELECT * FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    /// Mark a recording as stopped with its active (pause-excluded) duration.
    pub async fn mark_stopped(
        pool: &SqlitePool,
        meeting_id: &str,
        duration_seconds: Option<f64>,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE meetings SET status = ?, duration_seconds = ?, updated_at = ? WHERE id = ?",
        )
        .bind(meeting_status::STOPPED)
        .bind(duration_seconds)
        .bind(now_rfc3339())
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Meetings still `recording` belong to a previous app run that died: mark them
    /// `interrupted` and return them.
    pub async fn interrupt_unfinished(pool: &SqlitePool) -> Result<Vec<Meeting>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let unfinished = sqlx::query_as::<_, Meeting>("SELECT * FROM meetings WHERE status = ?")
            .bind(meeting_status::RECORDING)
            .fetch_all(&mut *tx)
            .await?;
        sqlx::query("UPDATE meetings SET status = ?, updated_at = ? WHERE status = ?")
            .bind(meeting_status::INTERRUPTED)
            .bind(now_rfc3339())
            .bind(meeting_status::RECORDING)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(unfinished)
    }

    /// Summaries may name timestamp placeholders; user titles stay authoritative.
    pub async fn update_generated_title(pool: &SqlitePool, meeting_id: &str, title: &str) -> Result<bool, sqlx::Error> {
        let Some(meeting) = Self::get(pool, meeting_id).await? else { return Ok(false) };
        let placeholder = ["Call ", "Meeting "].iter().any(|prefix| {
            meeting.title.strip_prefix(prefix).is_some_and(|date| chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M").is_ok())
        });
        if !placeholder { return Ok(false); }
        let result = sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ? AND title = ?")
            .bind(title).bind(now_rfc3339()).bind(meeting_id).bind(&meeting.title).execute(pool).await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn update_title(pool: &SqlitePool, meeting_id: &str, title: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
            .bind(title)
            .bind(now_rfc3339())
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Record the final-transcript state of a meeting (every `final_*` column at once).
    pub async fn set_final_state(pool: &SqlitePool, meeting_id: &str, state: &FinalState<'_>) -> Result<bool, sqlx::Error> {
        let now = now_rfc3339();
        let result = sqlx::query(
            "UPDATE meetings SET final_status = ?, final_model = ?, final_job_id = ?, final_error = ?,
                    final_updated_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(state.status)
        .bind(state.model)
        .bind(state.job_id)
        .bind(state.error)
        .bind(&now)
        .bind(&now)
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Meetings whose final transcript was queued or running when the app last exited.
    pub async fn final_in_progress(pool: &SqlitePool) -> Result<Vec<Meeting>, sqlx::Error> {
        sqlx::query_as::<_, Meeting>(
            "SELECT * FROM meetings WHERE final_status IN ('queued', 'uploading', 'transcribing')",
        )
        .fetch_all(pool)
        .await
    }

    /// Live sessions of a finished recording: public model, mode and realtime session ids (JSON array).
    pub async fn set_live_info(
        pool: &SqlitePool,
        meeting_id: &str,
        model: &str,
        mode: &str,
        session_ids_json: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE meetings SET live_model = ?, live_mode = ?, realtime_session_ids = ?, updated_at = ? WHERE id = ?",
        )
        .bind(model)
        .bind(mode)
        .bind(session_ids_json)
        .bind(now_rfc3339())
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Delete a meeting; transcripts and summary rows go with it (ON DELETE CASCADE).
    pub async fn delete(pool: &SqlitePool, meeting_id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_pool;

    async fn seed(pool: &SqlitePool, id: &str, status: &str) {
        MeetingsRepository::create(
            pool,
            &NewMeeting { id, title: "Call", folder_path: "/tmp/call", status },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn summaries_name_placeholders_and_preserve_custom_titles() {
        let pool = test_pool().await;
        seed(&pool, "title-test", meeting_status::STOPPED).await;
        assert!(!MeetingsRepository::update_generated_title(&pool, "title-test", "AI title").await.unwrap());
        MeetingsRepository::update_title(&pool, "title-test", "Call 2026-11-13 10:00").await.unwrap();
        assert!(MeetingsRepository::update_generated_title(&pool, "title-test", "AI title").await.unwrap());
        assert!(!MeetingsRepository::update_generated_title(&pool, "title-test", "Another AI title").await.unwrap());
        assert_eq!(MeetingsRepository::get(&pool, "title-test").await.unwrap().unwrap().title, "AI title");
    }

    #[tokio::test]
    async fn create_get_title_and_stop() {
        let pool = test_pool().await;
        seed(&pool, "m1", meeting_status::RECORDING).await;

        let meeting = MeetingsRepository::get(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(meeting.status, "recording");
        assert_eq!(meeting.folder_path.as_deref(), Some("/tmp/call"));

        assert!(MeetingsRepository::update_title(&pool, "m1", "Renamed").await.unwrap());
        assert!(MeetingsRepository::mark_stopped(&pool, "m1", Some(61.5)).await.unwrap());
        let meeting = MeetingsRepository::get(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(meeting.title, "Renamed");
        assert_eq!(meeting.status, "stopped");
        assert_eq!(meeting.duration_seconds, Some(61.5));

        assert!(!MeetingsRepository::update_title(&pool, "missing", "x").await.unwrap());
    }

    #[tokio::test]
    async fn interrupt_unfinished_only_touches_recording_rows() {
        let pool = test_pool().await;
        seed(&pool, "live", meeting_status::RECORDING).await;
        seed(&pool, "done", meeting_status::STOPPED).await;

        let interrupted = MeetingsRepository::interrupt_unfinished(&pool).await.unwrap();
        assert_eq!(interrupted.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["live"]);
        let live = MeetingsRepository::get(&pool, "live").await.unwrap().unwrap();
        let done = MeetingsRepository::get(&pool, "done").await.unwrap().unwrap();
        assert_eq!(live.status, "interrupted");
        assert_eq!(done.status, "stopped");
        assert!(MeetingsRepository::interrupt_unfinished(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_cascades_to_transcripts_and_summary() {
        let pool = test_pool().await;
        seed(&pool, "m1", meeting_status::STOPPED).await;
        sqlx::query("INSERT INTO transcripts (id, meeting_id, source, transcript) VALUES ('t1', 'm1', 'live', 'hi')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, status) VALUES ('m1', 'completed')")
            .execute(&pool)
            .await
            .unwrap();

        assert!(MeetingsRepository::delete(&pool, "m1").await.unwrap());
        let remaining: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM transcripts) + (SELECT COUNT(*) FROM summary_processes)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0);
        assert!(!MeetingsRepository::delete(&pool, "m1").await.unwrap());
    }

    #[tokio::test]
    async fn final_state_round_trips() {
        let pool = test_pool().await;
        seed(&pool, "m1", meeting_status::STOPPED).await;
        seed(&pool, "m2", meeting_status::STOPPED).await;
        let state = FinalState { status: "transcribing", model: "elevenlabs-scribe_v2", job_id: Some("job_1"), error: None };
        assert!(MeetingsRepository::set_final_state(&pool, "m1", &state).await.unwrap());
        let meeting = MeetingsRepository::get(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(meeting.final_status.as_deref(), Some("transcribing"));
        assert_eq!(meeting.final_job_id.as_deref(), Some("job_1"));
        assert!(meeting.final_updated_at.is_some());
        let running = MeetingsRepository::final_in_progress(&pool).await.unwrap();
        assert_eq!(running.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["m1"]);

        let failed = FinalState { status: "failed", error: Some("boom"), ..state };
        MeetingsRepository::set_final_state(&pool, "m1", &failed).await.unwrap();
        assert!(MeetingsRepository::final_in_progress(&pool).await.unwrap().is_empty());
        assert!(!MeetingsRepository::set_final_state(&pool, "missing", &failed).await.unwrap());
    }

    #[tokio::test]
    async fn status_is_constrained() {
        let pool = test_pool().await;
        let result = MeetingsRepository::create(
            &pool,
            &NewMeeting { id: "bad", title: "x", folder_path: "/tmp", status: "finished" },
        )
        .await;
        assert!(result.is_err());
    }
}
