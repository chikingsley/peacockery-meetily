-- 1 = every live channel transcribed all its audio on live_model; NULL = unknown (older rows, no live run).
ALTER TABLE meetings ADD COLUMN live_complete INTEGER;
