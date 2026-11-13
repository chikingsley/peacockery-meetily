import type {
  MeetingMetadata,
  MeetingTranscriptsPage,
  Transcript,
} from "../src/types/voice";

export const meetingMetadata = (
  id: string,
  overrides: Partial<MeetingMetadata> = {}
): MeetingMetadata => ({
  audio_path: null,
  created_at: "2026-11-01T10:00:00Z",
  duration_seconds: 1800,
  final_error: null,
  final_job_id: null,
  final_model: null,
  final_status: null,
  final_updated_at: null,
  folder_path: null,
  has_final: false,
  has_live: true,
  id,
  live_mode: "split",
  live_model: "deepgram-nova-3",
  realtime_session_ids: null,
  status: "stopped",
  title: id,
  updated_at: "2026-11-01T10:30:00Z",
  ...overrides,
});

export const transcriptRow = (
  text: string,
  overrides: Partial<Transcript> = {}
): Transcript => ({
  audio_end_time: 1,
  audio_start_time: 0,
  duration: 1,
  id: text,
  source: "live",
  speaker: null,
  text,
  timestamp: "10:00:00",
  ...overrides,
});

export const transcriptPage = (
  rows: Transcript[],
  hasMore = false
): MeetingTranscriptsPage => ({
  has_more: hasMore,
  source: rows[0]?.source ?? "live",
  total_count: hasMore ? rows.length + 1 : rows.length,
  transcripts: rows,
});
