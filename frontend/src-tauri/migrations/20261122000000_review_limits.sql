ALTER TABLE meeting_reviews ADD COLUMN attempted_revision TEXT;
ALTER TABLE meeting_reviews ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;

-- A switchboard and a person can share one diarization label. Apply the role
-- only to the turn that contains both a switchboard greeting and menu prompts.
ALTER TABLE transcript_edits ADD COLUMN speaker TEXT;
DROP VIEW named_transcripts;
CREATE VIEW named_transcripts AS
SELECT t.id,t.meeting_id,t.source,
 CASE WHEN e.speaker IS NOT NULL THEN e.speaker WHEN
  (lower(t.transcript) LIKE '%thank you for calling%' OR lower(t.transcript) LIKE '%menu options%')
  AND (lower(t.transcript) LIKE '%press one%' OR lower(t.transcript) LIKE '%press 1%'
       OR lower(t.transcript) LIKE '%enter the extension%')
 THEN 'Automated operator' ELSE coalesce(n.name,t.speaker) END AS speaker,
 coalesce(e.text,t.transcript) AS transcript,t.timestamp,t.audio_start_time,t.audio_end_time,t.duration,t.sequence
FROM transcripts t LEFT JOIN speaker_names n
 ON (n.meeting_id,n.source,n.label)=(t.meeting_id,t.source,t.speaker)
LEFT JOIN transcript_edits e ON e.transcript_id=t.id;

CREATE TABLE text_request_schedule (
    id INTEGER PRIMARY KEY CHECK (id=1),
    eligible_ms INTEGER NOT NULL
);
INSERT INTO text_request_schedule VALUES(1,0);
