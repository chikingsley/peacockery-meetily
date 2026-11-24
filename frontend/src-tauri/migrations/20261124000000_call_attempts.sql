CREATE TABLE call_destinations (
    meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    task_key TEXT NOT NULL,
    name TEXT,
    number TEXT,
    manual_outcome TEXT CHECK(manual_outcome IN ('conversation','voicemail_left','no_answer','undetermined'))
);
CREATE INDEX call_destinations_task ON call_destinations(task_key);
