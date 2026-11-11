import type { MeetingMetadata, MeetingTranscriptsPage, Transcript } from '../src/types/voice';

export const meetingMetadata = (id: string, overrides: Partial<MeetingMetadata> = {}): MeetingMetadata => ({
  id,
  title: id,
  created_at: '2026-11-01T10:00:00Z',
  updated_at: '2026-11-01T10:30:00Z',
  folder_path: null,
  audio_path: null,
  status: 'stopped',
  duration_seconds: 1800,
  live_model: 'deepgram-nova-3',
  live_mode: 'split',
  realtime_session_ids: null,
  final_status: null,
  final_model: null,
  final_job_id: null,
  final_error: null,
  final_updated_at: null,
  has_live: true,
  has_final: false,
  ...overrides,
});

export const transcriptRow = (text: string, overrides: Partial<Transcript> = {}): Transcript => ({
  id: text,
  text,
  timestamp: '10:00:00',
  speaker: null,
  source: 'live',
  audio_start_time: 0,
  audio_end_time: 1,
  duration: 1,
  ...overrides,
});

export const transcriptPage = (rows: Transcript[], hasMore = false): MeetingTranscriptsPage => ({
  transcripts: rows,
  total_count: hasMore ? rows.length + 1 : rows.length,
  has_more: hasMore,
  source: rows[0]?.source ?? 'live',
});
