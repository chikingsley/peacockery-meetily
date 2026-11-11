-- Peacockery Meetily schema (ARCHITECTURE.md §7). Timestamps are RFC 3339 UTC text.

CREATE TABLE meetings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT,
    folder_path TEXT,
    status TEXT NOT NULL DEFAULT 'recording'
        CHECK (status IN ('recording', 'stopped', 'interrupted', 'imported')),
    duration_seconds REAL,
    live_model TEXT,
    live_mode TEXT,
    realtime_session_ids TEXT,          -- JSON array of realtime session ids
    final_status TEXT,
    final_model TEXT,
    final_job_id TEXT,
    final_error TEXT,
    final_updated_at TEXT
);

CREATE TABLE transcripts (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('live', 'final')),
    speaker TEXT,
    transcript TEXT NOT NULL,
    timestamp TEXT,
    audio_start_time REAL,
    audio_end_time REAL,
    duration REAL,
    sequence INTEGER
);

CREATE INDEX idx_transcripts_meeting_source_start
    ON transcripts (meeting_id, source, audio_start_time);

CREATE TABLE summary_processes (
    meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    status TEXT,
    created_at TEXT,
    updated_at TEXT,
    error TEXT,
    result TEXT,
    start_time TEXT,
    end_time TEXT,
    processing_time REAL,
    metadata TEXT,
    result_backup TEXT,
    result_backup_timestamp TEXT
);
