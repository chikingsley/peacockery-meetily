-- Existing review receipts retain their keys. A full replacement can reuse both
-- the transcript row count and SQLite rowids, so distinguish later source generations.
ALTER TABLE meetings ADD COLUMN review_source_generation INTEGER NOT NULL DEFAULT 0;
CREATE VIEW meeting_review_candidates AS
SELECT m.id,m.status,m.created_at,(CASE WHEN m.status='recording' THEN
        CASE WHEN EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id=m.id
            AND t.source IN ('live','final') AND t.audio_start_time>=300) THEN 'live:1' ELSE 'live:0' END
        ELSE 'end:'||m.review_transcript_count||':'||(
            SELECT MAX(t.rowid) FROM transcripts t WHERE t.meeting_id=m.id
        ) END) || CASE WHEN m.review_source_generation>0
            THEN ':r'||m.review_source_generation ELSE '' END AS revision
    FROM meetings m WHERE m.review_transcript_count>0;
