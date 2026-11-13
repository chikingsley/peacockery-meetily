/**
 * Types for every payload of the Rust <-> frontend contract (ARCHITECTURE.md §2, §8, §9).
 * Field names match the Rust serde output exactly (snake_case).
 */

// ---------------------------------------------------------------------------
// Settings, credential, models (§2, §8 "Voice settings")
// ---------------------------------------------------------------------------

export type LiveMode = "split" | "mixed";

export interface VoiceSettings {
  base_url: string;
  live_enabled: boolean;
  live_model: string;
  live_mode: LiveMode;
  live_diarize: boolean;
  final_model: string;
  final_diarize: boolean;
  auto_final: boolean;
  /** ISO-639-1 code, or "auto". */
  language: string;
  keyterms: string[];
  me_label: string;
  them_label: string;
  summary_model: string;
  summary_fallback_models: string[];
  auto_summary: boolean;
}

export interface ModelOption {
  id: string;
  label: string;
  note: string;
  available: boolean;
  supports_diarization: boolean;
  recommended: boolean;
}

export interface VoiceModels {
  live: ModelOption[];
  final: ModelOption[];
  summary: ModelOption[];
  /** Why the server's model list could not be read (the lists then show the built-in catalog). */
  error: string | null;
}

export interface KeyStatus {
  configured: boolean;
  masked: string | null;
}

export interface ConnectionTestResult {
  ok: boolean;
  environment: string | null;
  latency_ms: number;
  error: string | null;
}

// ---------------------------------------------------------------------------
// Recording (§8 "Recording")
// ---------------------------------------------------------------------------

export type AudioDeviceType = "Input" | "Output";

export interface AudioDevice {
  name: string;
  device_type: AudioDeviceType;
  /** Display label; set for pseudo devices such as `app:chrome`. */
  label?: string | null;
}

/** Pseudo system device: process-tree loopback of Chrome (Google Voice PWA). */
export const CHROME_APP_DEVICE = "app:chrome";

export interface StartRecordingResult {
  meeting_id: string;
  folder_path: string;
}

export interface StopRecordingResult {
  meeting_id: string;
  folder_path: string;
  /** Null when the audio could not be saved (the reason arrives as `recording-error`). */
  audio_path: string | null;
}

export interface RecordingState {
  is_recording: boolean;
  is_paused: boolean;
  is_active: boolean;
  /** True from the stop request until `recording-stopped`. */
  is_stopping: boolean;
  meeting_id: string | null;
  recording_duration: number | null;
  active_duration: number | null;
}

export interface RecordingPreferences {
  save_folder: string;

  preferred_mic_device: string | null;
  preferred_system_device: string | null;
}

// ---------------------------------------------------------------------------
// Meetings and transcripts (§7, §8 "Meetings / transcripts")
// ---------------------------------------------------------------------------

export type TranscriptSource = "live" | "final";

export type TranscriptSourceQuery = "best" | TranscriptSource;

export type MeetingStatus =
  | "recording"
  | "stopped"
  | "interrupted"
  | "imported";

export interface MeetingListItem {
  id: string;
  title: string;
  created_at: string | null;
  final_status: FinalTranscriptState | null;
}

export interface MeetingMetadata {
  id: string;
  title: string;
  created_at: string | null;
  updated_at: string | null;
  folder_path: string | null;
  /** Absolute path of the meeting's audio file, null when there is none. */
  audio_path: string | null;
  status: MeetingStatus;
  duration_seconds: number | null;
  live_model: string | null;
  live_mode: LiveMode | null;
  /** JSON array of realtime session ids, as stored. */
  realtime_session_ids: string | null;
  final_status: FinalTranscriptState | null;
  final_model: string | null;
  final_job_id: string | null;
  final_error: string | null;
  final_updated_at: string | null;
  has_live: boolean;
  has_final: boolean;
}

export interface Transcript {
  id: string;
  text: string;
  /** Wall-clock "HH:MM:SS"; null for final-transcript rows. */
  timestamp: string | null;
  speaker: string | null;
  source: TranscriptSource;
  /** Seconds from recording start. */
  audio_start_time: number | null;
  audio_end_time: number | null;
  duration: number | null;
}

export interface MeetingTranscriptsPage {
  transcripts: Transcript[];
  total_count: number;
  has_more: boolean;
  /** Which rows were returned ("best" resolves to one of these). */
  source: TranscriptSource;
}

export interface TranscriptSearchResult {
  id: string;
  title: string;
  matchContext: string;
  timestamp: string | null;
}

// ---------------------------------------------------------------------------
// Final transcript (§6)
// ---------------------------------------------------------------------------

export type FinalTranscriptState =
  | "queued"
  | "uploading"
  | "transcribing"
  | "done"
  | "failed";

export interface FinalTranscriptStatusResult {
  state: FinalTranscriptState | null;
  model: string | null;
  error: string | null;
}

// ---------------------------------------------------------------------------
// Import (§8 "Import")
// ---------------------------------------------------------------------------

export interface AudioFileInfo {
  path: string;
  filename: string;
  duration_seconds: number;
  size_bytes: number;
  format: string;
}

export interface StartImportResult {
  meeting_id: string;
}

// ---------------------------------------------------------------------------
// Summaries (§8 "Summaries")
// ---------------------------------------------------------------------------

export interface ProcessTranscriptResult {
  process_id: string;
}

export interface SummaryTemplate {
  id: string;
  name: string;
  description: string;
}

// ---------------------------------------------------------------------------
// Events (§9)
// ---------------------------------------------------------------------------

export type LiveChannel = "mic" | "system" | "mixed";

export interface RecordingStartedEvent {
  meeting_id: string;
  folder_path: string;
}

export interface RecordingStoppingEvent {
  meeting_id: string;
}

export interface RecordingStoppedEvent {
  meeting_id: string;
  folder_path: string;
  meeting_name: string;
}

/** `transcript-update`: one finalized live line. Also the shape of `get_transcript_history` rows. */
export interface TranscriptUpdate {
  meeting_id: string;
  /** Restarts at 0 per recording; unique within it. */
  sequence_id: number;
  text: string;
  speaker: string | null;
  channel: LiveChannel;
  /** Local wall-clock "HH:MM:SS". */
  timestamp: string;
  audio_start_time: number;
  audio_end_time: number;
  duration: number;
  is_partial: false;
  source: "live";
  confidence: null;
}

export type LiveLine = TranscriptUpdate;

/** `transcript-interim`: replaces that channel's preview line; `text: ""` clears it. */
export interface TranscriptInterim {
  meeting_id: string;
  channel: LiveChannel;
  speaker: string | null;
  text: string;
}

export type LiveTranscriptionState =
  | "connecting"
  | "live"
  | "reconnecting"
  | "failed"
  | "closed";

export interface LiveTranscriptionStatus {
  meeting_id: string;
  channel: LiveChannel;
  model: string;
  state: LiveTranscriptionState;
  message: string | null;
}

/** `audio-levels`: levels 0..1 (dBFS mapped -60..0), ~10 Hz. */
export interface AudioLevels {
  mic: number;
  system: number;
  system_silent_seconds: number;
}

/** `final-transcript-status`. */
export interface FinalTranscriptStatus {
  meeting_id: string;
  state: FinalTranscriptState;
  model: string;
  message: string | null;
  segments_count: number | null;
}

export interface MicDeviceSwitchedEvent {
  device_name: string;
}

export interface MicSwapFailedEvent {
  error: string;
  device_name: string;
}

export interface MicRecoveryExhaustedEvent {
  device_name: string;
}

export interface RecordingErrorEvent {
  message: string;
}

export interface ImportProgressEvent {
  meeting_id: string;
  stage: string;
  progress_percentage: number;
  message: string;
}

export interface ImportCompleteEvent {
  meeting_id: string;
}

export interface ImportErrorEvent {
  meeting_id: string;
  error: string;
}
