ALTER TABLE meeting_reviews ADD COLUMN brief_key TEXT;
CREATE INDEX meeting_reviews_brief ON meeting_reviews(brief_key);

-- Reviewed spellings are scoped to a transcript turn and expire with that source.
CREATE TABLE transcript_edits (
    transcript_id TEXT PRIMARY KEY REFERENCES transcripts(id) ON DELETE CASCADE,
    text TEXT NOT NULL
);
DROP VIEW named_transcripts;
CREATE VIEW named_transcripts AS
SELECT t.id,t.meeting_id,t.source,coalesce(n.name,t.speaker) AS speaker,coalesce(e.text,t.transcript) AS transcript,
       t.timestamp,t.audio_start_time,t.audio_end_time,t.duration,t.sequence
FROM transcripts t LEFT JOIN speaker_names n
ON (n.meeting_id,n.source,n.label)=(t.meeting_id,t.source,t.speaker)
LEFT JOIN transcript_edits e ON e.transcript_id=t.id;
