-- Review is a presentation layer; the original diarization and transcript remain available.
CREATE TABLE speaker_names (
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    label TEXT NOT NULL,
    name TEXT NOT NULL,
    evidence TEXT NOT NULL,
    manual INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(meeting_id,source,label)
);
CREATE VIEW named_transcripts AS
SELECT t.id,t.meeting_id,t.source,coalesce(n.name,t.speaker) AS speaker,t.transcript,
       t.timestamp,t.audio_start_time,t.audio_end_time,t.duration,t.sequence
FROM transcripts t LEFT JOIN speaker_names n
ON (n.meeting_id,n.source,n.label)=(t.meeting_id,t.source,t.speaker);
CREATE TABLE meeting_reviews (
    meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    revision TEXT NOT NULL DEFAULT '',
    outcomes TEXT NOT NULL DEFAULT '[]',
    model TEXT,
    retry_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE meeting_briefs (
    item_key TEXT PRIMARY KEY,
    notes TEXT NOT NULL,
    version INTEGER NOT NULL
);
ALTER TABLE meeting_publications ADD COLUMN notes_check_at INTEGER NOT NULL DEFAULT 0;
