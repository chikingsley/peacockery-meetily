-- Keep the review worker's count and last-row lookup independent of transcript length.
ALTER TABLE meetings ADD COLUMN review_transcript_count INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_transcripts_meeting ON transcripts(meeting_id);
UPDATE meetings SET review_transcript_count = (
    SELECT count(*) FROM transcripts WHERE meeting_id = meetings.id
);

CREATE TRIGGER review_transcript_insert AFTER INSERT ON transcripts BEGIN
    UPDATE meetings SET review_transcript_count = review_transcript_count + 1 WHERE id = NEW.meeting_id;
END;
CREATE TRIGGER review_transcript_delete AFTER DELETE ON transcripts BEGIN
    UPDATE meetings SET review_transcript_count = review_transcript_count - 1 WHERE id = OLD.meeting_id;
END;
CREATE TRIGGER review_transcript_move AFTER UPDATE OF meeting_id ON transcripts
WHEN NEW.meeting_id != OLD.meeting_id BEGIN
    UPDATE meetings SET review_transcript_count = review_transcript_count - 1 WHERE id = OLD.meeting_id;
    UPDATE meetings SET review_transcript_count = review_transcript_count + 1 WHERE id = NEW.meeting_id;
END;
