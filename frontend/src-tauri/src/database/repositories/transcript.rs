use crate::database::models::{transcript_source, NewTranscript, TranscriptRow};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Which transcript rows of a meeting to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceChoice {
    /// `final` when the meeting has final rows, else `live`.
    Best,
    Live,
    Final,
}

impl SourceChoice {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("best") {
            "best" => Ok(Self::Best),
            "live" => Ok(Self::Live),
            "final" => Ok(Self::Final),
            other => Err(format!(
                "Unknown transcript source '{}' (best|live|final)",
                other
            )),
        }
    }
}

/// A search hit: the meeting and a snippet around the first match.
#[derive(Debug, Serialize)]
pub struct TranscriptSearchResult {
    pub id: String,
    pub title: String,
    #[serde(rename = "matchContext")]
    pub match_context: String,
    pub timestamp: Option<String>,
}

/// Carry an explicitly named voice across a new diarization only when at least three
/// turns independently overlap the same person's speech and cover most of the cluster.
fn aligned_names(lines: &[NewTranscript], anchors: &[(String, f64, f64)]) -> Vec<(String, String)> {
    use std::collections::BTreeMap;
    let mut lengths = BTreeMap::<&str, f64>::new();
    let mut votes = BTreeMap::<(&str, &str), (usize, f64)>::new();
    for line in lines {
        let (Some(label), Some(start), Some(end)) = (
            line.speaker.as_deref(),
            line.audio_start_time,
            line.audio_end_time,
        ) else {
            continue;
        };
        if !(end > start && start.is_finite() && end.is_finite()) {
            continue;
        }
        *lengths.entry(label).or_default() += end - start;
        let mut overlaps = BTreeMap::<&str, f64>::new();
        for (name, a, b) in anchors.iter().take_while(|(_, a, _)| *a < end) {
            let overlap = end.min(*b) - start.max(*a);
            if overlap > 0.0 {
                *overlaps.entry(name).or_default() += overlap;
            }
        }
        // Crosstalk or mixed-channel speech cannot establish one person's identity.
        if overlaps.len() != 1 {
            continue;
        }
        if let Some((name, overlap)) = overlaps.first_key_value() {
            if *overlap >= (end - start) * 0.75 {
                let vote = votes.entry((label, name)).or_default();
                vote.0 += 1;
                vote.1 += overlap.min(end - start);
            }
        }
    }
    votes
        .into_iter()
        .filter(|&((label, _), (turns, seconds))| {
            turns >= 3 && seconds >= 8.0 && seconds >= 0.85 * lengths[label]
        })
        .map(|((label, name), _)| (label.to_owned(), name.to_owned()))
        .collect()
}

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Number of rows per source for one meeting: `(live, final)`.
    pub async fn source_counts(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<(i64, i64), sqlx::Error> {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT
                COALESCE(SUM(source = 'live'), 0),
                COALESCE(SUM(source = 'final'), 0)
             FROM transcripts WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await
    }

    /// Resolve `choice` to a concrete source (`live` or `final`).
    pub async fn resolve_source(
        pool: &SqlitePool,
        meeting_id: &str,
        choice: SourceChoice,
    ) -> Result<&'static str, sqlx::Error> {
        Ok(match choice {
            SourceChoice::Live => transcript_source::LIVE,
            SourceChoice::Final => transcript_source::FINAL,
            SourceChoice::Best => {
                let (_, final_count) = Self::source_counts(pool, meeting_id).await?;
                if final_count > 0 {
                    transcript_source::FINAL
                } else {
                    transcript_source::LIVE
                }
            }
        })
    }

    /// One page of a meeting's rows of `source`, in audio order, plus the total count.
    pub async fn page(
        pool: &SqlitePool,
        meeting_id: &str,
        source: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<TranscriptRow>, i64), sqlx::Error> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND source = ?",
        )
        .bind(meeting_id)
        .bind(source)
        .fetch_one(pool)
        .await?;
        let rows = sqlx::query_as::<_, TranscriptRow>(
            "SELECT * FROM named_transcripts WHERE meeting_id = ? AND source = ?
             ORDER BY audio_start_time ASC, sequence ASC LIMIT ? OFFSET ?",
        )
        .bind(meeting_id)
        .bind(source)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
        Ok((rows, total))
    }

    /// Every row of `source`, in audio order.
    pub async fn all(
        pool: &SqlitePool,
        meeting_id: &str,
        source: &str,
    ) -> Result<Vec<TranscriptRow>, sqlx::Error> {
        sqlx::query_as::<_, TranscriptRow>(
            "SELECT * FROM named_transcripts WHERE meeting_id = ? AND source = ?
             ORDER BY audio_start_time ASC, sequence ASC",
        )
        .bind(meeting_id)
        .bind(source)
        .fetch_all(pool)
        .await
    }

    /// Insert one row of `source`.
    pub async fn insert(
        pool: &SqlitePool,
        meeting_id: &str,
        source: &str,
        line: &NewTranscript,
    ) -> Result<String, sqlx::Error> {
        let id = format!("transcript-{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO transcripts
                (id, meeting_id, source, speaker, transcript, timestamp, audio_start_time, audio_end_time, duration, sequence)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(meeting_id)
        .bind(source)
        .bind(&line.speaker)
        .bind(&line.text)
        .bind(&line.timestamp)
        .bind(line.audio_start_time)
        .bind(line.audio_end_time)
        .bind(line.duration)
        .bind(line.sequence)
        .execute(pool)
        .await?;
        Ok(id)
    }

    /// Replace all rows of `source` for a meeting in one transaction (other sources stay).
    pub async fn replace_source(
        pool: &SqlitePool,
        meeting_id: &str,
        source: &str,
        lines: &[NewTranscript],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        let anchors:Vec<(String,f64,f64)>=sqlx::query_as(
            "SELECT n.name,t.audio_start_time,t.audio_end_time FROM transcripts t
             JOIN speaker_names n ON (t.meeting_id,t.source,t.speaker)=(n.meeting_id,n.source,n.label)
             WHERE t.meeting_id=? AND t.source=? AND n.manual=1 AND t.audio_end_time>t.audio_start_time ORDER BY t.audio_start_time")
            .bind(meeting_id).bind(source).fetch_all(&mut *tx).await?;
        let mut aligned = aligned_names(lines, &anchors);
        if source == "final" {
            let live:Vec<(String,f64,f64)>=sqlx::query_as(
                "SELECT n.name,t.audio_start_time,t.audio_end_time FROM transcripts t JOIN speaker_names n
                 ON (t.meeting_id,t.source,t.speaker)=(n.meeting_id,n.source,n.label)
                 WHERE t.meeting_id=? AND t.source='live' AND n.manual=1 AND t.audio_end_time>t.audio_start_time ORDER BY t.audio_start_time")
                 .bind(meeting_id).fetch_all(&mut *tx).await?;
            for pair in aligned_names(lines, &live) {
                if !aligned.iter().any(|(label, _)| label == &pair.0) {
                    aligned.push(pair);
                }
            }
        }
        // Provider clusters can be renumbered on a rerun. A fresh source gets a fresh review.
        sqlx::query("DELETE FROM speaker_names WHERE meeting_id=? AND source=?")
            .bind(meeting_id)
            .bind(source)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE meeting_reviews SET revision='', retry_at=0 WHERE meeting_id=?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM transcripts WHERE meeting_id = ? AND source = ?")
            .bind(meeting_id)
            .bind(source)
            .execute(&mut *tx)
            .await?;
        for line in lines {
            sqlx::query(
                "INSERT INTO transcripts
                    (id, meeting_id, source, speaker, transcript, timestamp, audio_start_time, audio_end_time, duration, sequence)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(format!("transcript-{}", Uuid::new_v4()))
            .bind(meeting_id)
            .bind(source)
            .bind(&line.speaker)
            .bind(&line.text)
            .bind(&line.timestamp)
            .bind(line.audio_start_time)
            .bind(line.audio_end_time)
            .bind(line.duration)
            .bind(line.sequence)
            .execute(&mut *tx)
            .await?;
        }
        for (label, name) in aligned {
            sqlx::query("INSERT INTO speaker_names(meeting_id,source,label,name,evidence,manual) VALUES(?,?,?,?,'Timed overlap with manually named speech',1)")
                .bind(meeting_id).bind(source).bind(label).bind(name).execute(&mut *tx).await?;
        }
        tx.commit().await
    }

    /// Rename a speaker on every row (live and final) of a meeting. Returns rows changed.
    pub async fn rename_speaker(
        pool: &SqlitePool,
        meeting_id: &str,
        from: &str,
        to: &str,
    ) -> Result<u64, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM named_transcripts WHERE meeting_id=? AND speaker=?",
        )
        .bind(meeting_id)
        .bind(from)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO speaker_names(meeting_id,source,label,name,evidence,manual)
            SELECT DISTINCT t.meeting_id,t.source,t.speaker,?,'Manual rename',1
            FROM transcripts t JOIN named_transcripts n ON n.id=t.id
            WHERE t.meeting_id=? AND n.speaker=?
            ON CONFLICT(meeting_id,source,label) DO UPDATE SET name=excluded.name,evidence=excluded.evidence,manual=1")
            .bind(to).bind(meeting_id).bind(from).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(count as u64)
    }

    /// Literal substring search over the best transcript, one hit per meeting, newest first.
    pub async fn search(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, sqlx::Error> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let escaped = query
            .trim()
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{}%", escaped);
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "WITH matches AS (
                SELECT m.id, m.title, m.created_at, t.transcript, t.timestamp,
                       ROW_NUMBER() OVER (PARTITION BY m.id ORDER BY t.audio_start_time, t.sequence) AS hit
                FROM meetings m JOIN transcripts t ON m.id = t.meeting_id
                WHERE LOWER(t.transcript) LIKE ? ESCAPE '\\'
                  AND (t.source = 'final' OR NOT EXISTS (
                    SELECT 1 FROM transcripts f WHERE f.meeting_id = m.id AND f.source = 'final'
                  ))
             )
             SELECT id, title, transcript, timestamp FROM matches WHERE hit = 1
             ORDER BY created_at DESC, id LIMIT 100",
        )
        .bind(&pattern)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, title, transcript, timestamp)| TranscriptSearchResult {
                    id,
                    title,
                    match_context: match_context(&transcript, query.trim()),
                    timestamp,
                },
            )
            .collect())
    }
}

/// About 100 characters on each side of the first match.
fn match_context(transcript: &str, query: &str) -> String {
    let chars: Vec<char> = transcript.chars().collect();
    let lower: Vec<char> = transcript.to_lowercase().chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let found = (lower.len() == chars.len())
        .then(|| {
            lower
                .windows(needle.len().max(1))
                .position(|w| w == needle.as_slice())
        })
        .flatten();
    match found {
        Some(index) => {
            let start = index.saturating_sub(100);
            let end = (index + needle.len() + 100).min(chars.len());
            let mut context = String::new();
            if start > 0 {
                context.push_str("...");
            }
            context.extend(&chars[start..end]);
            if end < chars.len() {
                context.push_str("...");
            }
            context
        }
        None => chars.iter().take(200).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::meeting_status;
    use crate::database::repositories::meeting::{MeetingsRepository, NewMeeting};
    use crate::database::test_pool;

    fn line(speaker: &str, text: &str, start: f64) -> NewTranscript {
        NewTranscript {
            speaker: Some(speaker.to_string()),
            text: text.to_string(),
            timestamp: None,
            audio_start_time: Some(start),
            audio_end_time: Some(start + 1.0),
            duration: Some(1.0),
            sequence: None,
        }
    }

    async fn pool_with_meeting() -> SqlitePool {
        let pool = test_pool().await;
        MeetingsRepository::create(
            &pool,
            &NewMeeting {
                id: "m1",
                title: "Call",
                folder_path: "/tmp",
                status: meeting_status::STOPPED,
            },
        )
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn best_source_prefers_final_rows() {
        let pool = pool_with_meeting().await;
        TranscriptsRepository::insert(&pool, "m1", "live", &line("Me", "hello", 0.0))
            .await
            .unwrap();
        assert_eq!(
            TranscriptsRepository::resolve_source(&pool, "m1", SourceChoice::Best)
                .await
                .unwrap(),
            "live"
        );

        TranscriptsRepository::replace_source(
            &pool,
            "m1",
            "final",
            &[line("Them", "hi", 0.5), line("Me", "hello", 0.0)],
        )
        .await
        .unwrap();
        assert_eq!(
            TranscriptsRepository::source_counts(&pool, "m1")
                .await
                .unwrap(),
            (1, 2)
        );
        assert_eq!(
            TranscriptsRepository::resolve_source(&pool, "m1", SourceChoice::Best)
                .await
                .unwrap(),
            "final"
        );

        let (rows, total) = TranscriptsRepository::page(&pool, "m1", "final", 10, 0)
            .await
            .unwrap();
        assert_eq!(total, 2);
        assert_eq!(
            rows.iter()
                .map(|r| r.transcript.as_str())
                .collect::<Vec<_>>(),
            vec!["hello", "hi"]
        );

        // Replacing final rows keeps live rows.
        TranscriptsRepository::replace_source(&pool, "m1", "final", &[line("Me", "only", 0.0)])
            .await
            .unwrap();
        assert_eq!(
            TranscriptsRepository::source_counts(&pool, "m1")
                .await
                .unwrap(),
            (1, 1)
        );
    }

    #[tokio::test]
    async fn rename_speaker_touches_both_sources() {
        let pool = pool_with_meeting().await;
        TranscriptsRepository::insert(&pool, "m1", "live", &line("Them", "a", 0.0))
            .await
            .unwrap();
        TranscriptsRepository::insert(&pool, "m1", "final", &line("Them", "b", 0.0))
            .await
            .unwrap();
        TranscriptsRepository::insert(&pool, "m1", "final", &line("Me", "c", 1.0))
            .await
            .unwrap();
        assert_eq!(
            TranscriptsRepository::rename_speaker(&pool, "m1", "Them", "Dana")
                .await
                .unwrap(),
            2
        );
        let speakers: Vec<Option<String>> =
            sqlx::query_scalar("SELECT speaker FROM named_transcripts ORDER BY transcript")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            speakers,
            vec![Some("Dana".into()), Some("Dana".into()), Some("Me".into())]
        );
    }

    #[tokio::test]
    async fn search_returns_context() {
        let pool = pool_with_meeting().await;
        TranscriptsRepository::insert(
            &pool,
            "m1",
            "live",
            &line("Me", "We ship the Peacock build Friday", 0.0),
        )
        .await
        .unwrap();
        let hits = TranscriptsRepository::search(&pool, "peacock")
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Call");
        assert!(hits[0].match_context.contains("Peacock build"));
        assert!(TranscriptsRepository::search(&pool, "  ")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn search_is_literal_deduplicated_and_prefers_final() {
        let pool = pool_with_meeting().await;
        TranscriptsRepository::insert(&pool, "m1", "live", &line("Me", "old draft", 0.0))
            .await
            .unwrap();
        TranscriptsRepository::insert(
            &pool,
            "m1",
            "final",
            &line("Me", "100% ready_for review", 0.0),
        )
        .await
        .unwrap();
        TranscriptsRepository::insert(&pool, "m1", "final", &line("Me", "100% complete", 2.0))
            .await
            .unwrap();
        assert_eq!(
            TranscriptsRepository::search(&pool, "  100%  ")
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            TranscriptsRepository::search(&pool, "_")
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(TranscriptsRepository::search(&pool, "old draft")
            .await
            .unwrap()
            .is_empty());
        assert!(TranscriptsRepository::search(&pool, "ready%for")
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn source_choice_parses() {
        assert_eq!(SourceChoice::parse(None).unwrap(), SourceChoice::Best);
        assert_eq!(
            SourceChoice::parse(Some("final")).unwrap(),
            SourceChoice::Final
        );
        assert!(SourceChoice::parse(Some("both")).is_err());
    }

    #[test]
    fn match_context_handles_multibyte_text() {
        let text = format!("{}café meeting", "é".repeat(150));
        let context = match_context(&text, "CAFÉ");
        assert!(context.starts_with("..."));
        assert!(context.contains("café meeting"));
    }
}

#[cfg(test)]
mod alignment_tests {
    use super::*;
    #[test]
    fn renumbered_cluster_requires_clear_timed_agreement() {
        let lines: Vec<_> = (0..4)
            .map(|i| NewTranscript {
                speaker: Some("Speaker 7".into()),
                text: "test".into(),
                timestamp: None,
                audio_start_time: Some(i as f64 * 4.),
                audio_end_time: Some(i as f64 * 4. + 3.),
                duration: Some(3.),
                sequence: Some(i),
            })
            .collect();
        assert_eq!(
            aligned_names(&lines, &[("Dana".into(), 0., 18.)]),
            vec![("Speaker 7".into(), "Dana".into())]
        );
        assert!(
            aligned_names(&lines, &[("Dana".into(), 0., 9.), ("Lee".into(), 8., 18.)]).is_empty()
        );
    }
}
