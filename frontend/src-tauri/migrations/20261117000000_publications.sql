-- Only explicitly queued meetings enter this table; existing recordings stay local.
CREATE TABLE meeting_publications (
    meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'queued',
    upload_id TEXT,
    parts_json TEXT NOT NULL DEFAULT '[]',
    bytes_sent INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at INTEGER NOT NULL DEFAULT 0
);
