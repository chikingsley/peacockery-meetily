/**
 * The one wrapper around the Rust contract (ARCHITECTURE.md §8 commands, §9 events).
 * Tauri maps the camelCase argument keys below to the snake_case Rust parameters.
 */

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";

import type { SummaryDataResponse, SummaryProcessResponse } from "@/types";
import type {
  AudioDevice,
  AudioFileInfo,
  AudioLevels,
  ConnectionTestResult,
  FinalTranscriptStatus,
  FinalTranscriptStatusResult,
  ImportCompleteEvent,
  ImportErrorEvent,
  ImportProgressEvent,
  KeyStatus,
  LiveLine,
  LiveTranscriptionStatus,
  MeetingListItem,
  MeetingMetadata,
  MeetingTranscriptsPage,
  MicDeviceSwitchedEvent,
  MicRecoveryExhaustedEvent,
  MicSwapFailedEvent,
  ProcessTranscriptResult,
  RecordingErrorEvent,
  RecordingPreferences,
  RecordingStartedEvent,
  RecordingState,
  RecordingStoppedEvent,
  RecordingStoppingEvent,
  StartImportResult,
  StartRecordingResult,
  StopRecordingResult,
  SummaryTemplate,
  TranscriptInterim,
  TranscriptSearchResult,
  TranscriptSourceQuery,
  TranscriptUpdate,
  VoiceModels,
  VoiceSettings,
} from "@/types/voice";

/** Bound the start call so a hung native start settles into an error instead of a spinner. */
const START_TIMEOUT_MS = 120_000;

function withTimeout<T>(
  promise: Promise<T>,
  ms: number,
  message: string
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
    promise
      .then((value) => {
        clearTimeout(timer);
        resolve(value);
      })
      .catch((error) => {
        clearTimeout(timer);
        reject(error);
      });
  });
}

function on<T>(
  event: string,
  callback: (payload: T) => void
): Promise<UnlistenFn> {
  if (!isTauri()) {
    return Promise.resolve(() => {
      /* Browser preview has no native events. */
    });
  }

  return listen<T>(event, (e) => callback(e.payload));
}

export const voiceService = {
  // --- Recording ---------------------------------------------------------
  getAudioDevices: () => invoke<AudioDevice[]>("get_audio_devices"),
  startRecording: (
    micDeviceName: string | null,
    systemDeviceName: string | null,
    meetingName: string | null
  ) =>
    withTimeout(
      invoke<StartRecordingResult>("start_recording_with_devices_and_meeting", {
        meetingName,
        micDeviceName,
        systemDeviceName,
      }),
      START_TIMEOUT_MS,
      "Recording start timed out after 120s"
    ),
  stopRecording: () => invoke<StopRecordingResult>("stop_recording"),
  pauseRecording: () => invoke<void>("pause_recording"),
  resumeRecording: () => invoke<void>("resume_recording"),
  isRecording: () => invoke<boolean>("is_recording"),
  getRecordingState: () => invoke<RecordingState>("get_recording_state"),
  getRecordingMeetingName: () =>
    invoke<string | null>("get_recording_meeting_name"),

  getTranscriptHistory: () => invoke<LiveLine[]>("get_transcript_history"),
  getRecordingPreferences: () =>
    invoke<RecordingPreferences>("get_recording_preferences"),
  setRecordingPreferences: (preferences: RecordingPreferences) =>
    invoke<void>("set_recording_preferences", { preferences }),
  getDefaultRecordingsFolderPath: () =>
    invoke<string>("get_default_recordings_folder_path"),
  openRecordingsFolder: () => invoke<void>("open_recordings_folder"),
  startAudioLevelMonitoring: (deviceNames: string[]) =>
    invoke<void>("start_audio_level_monitoring", { deviceNames }),
  stopAudioLevelMonitoring: () => invoke<void>("stop_audio_level_monitoring"),

  // --- Voice settings ----------------------------------------------------
  getSettings: () => invoke<VoiceSettings>("voice_get_settings"),
  saveSettings: (settings: VoiceSettings) =>
    invoke<VoiceSettings>("voice_save_settings", { settings }),
  getKeyStatus: () => invoke<KeyStatus>("voice_get_key_status"),
  setApiKey: (key: string) => invoke<KeyStatus>("voice_set_api_key", { key }),
  clearApiKey: () => invoke<KeyStatus>("voice_clear_api_key"),
  testConnection: () => invoke<ConnectionTestResult>("voice_test_connection"),
  listModels: () => invoke<VoiceModels>("voice_list_models"),

  // --- Meetings / transcripts -------------------------------------------
  getMeetings: () => invoke<MeetingListItem[]>("api_get_meetings"),
  searchTranscripts: (query: string) =>
    invoke<TranscriptSearchResult[]>("api_search_transcripts", { query }),
  getMeetingMetadata: (meetingId: string) =>
    invoke<MeetingMetadata>("api_get_meeting_metadata", { meetingId }),
  getMeetingTranscripts: (
    meetingId: string,
    limit: number,
    offset: number,
    source: TranscriptSourceQuery = "best"
  ) =>
    invoke<MeetingTranscriptsPage>("api_get_meeting_transcripts", {
      limit,
      meetingId,
      offset,
      source,
    }),
  saveMeetingTitle: (meetingId: string, title: string) =>
    invoke<void>("api_save_meeting_title", { meetingId, title }),
  deleteMeeting: (meetingId: string) =>
    invoke<void>("api_delete_meeting", { meetingId }),
  openMeetingFolder: (meetingId: string) =>
    invoke<void>("open_meeting_folder", { meetingId }),
  renameSpeaker: (meetingId: string, from: string, to: string) =>
    invoke<number>("rename_speaker", { from, meetingId, to }),
  startFinalTranscript: (meetingId: string, model?: string | null) =>
    invoke<void>("start_final_transcript", { meetingId, model: model ?? null }),
  getFinalTranscriptStatus: (meetingId: string) =>
    invoke<FinalTranscriptStatusResult>("get_final_transcript_status", {
      meetingId,
    }),

  // --- Import ------------------------------------------------------------
  selectAndValidateAudio: () =>
    invoke<AudioFileInfo | null>("select_and_validate_audio_command"),
  validateAudioFile: (path: string) =>
    invoke<AudioFileInfo>("validate_audio_file_command", { path }),
  startImport: (sourcePath: string, title: string | null) =>
    invoke<StartImportResult>("start_import_audio_command", {
      sourcePath,
      title,
    }),
  cancelImport: () => invoke<void>("cancel_import_command"),

  // --- Summaries ---------------------------------------------------------
  processTranscript: (
    meetingId: string,
    templateId: string | null,
    customPrompt: string | null,
    model: string | null
  ) =>
    invoke<ProcessTranscriptResult>("api_process_transcript", {
      customPrompt,
      meetingId,
      model,
      templateId,
    }),
  getSummary: (meetingId: string) =>
    invoke<SummaryProcessResponse>("api_get_summary", { meetingId }),
  saveMeetingSummary: (
    meetingId: string,
    summary: SummaryDataResponse | Record<string, unknown>
  ) => invoke<void>("api_save_meeting_summary", { meetingId, summary }),
  cancelSummary: (meetingId: string, processId: string) =>
    invoke<void>("api_cancel_summary", { meetingId, processId }),
  listTemplates: () => invoke<SummaryTemplate[]>("api_list_templates"),

  // --- Events ------------------------------------------------------------
  /** Tray "Start Recording": the sidebar's start path handles it on every page. */
  onRequestRecordingToggle: (cb: () => void) =>
    on<unknown>("request-recording-toggle", () => cb()),
  onRecordingStarting: (cb: () => void) =>
    on<null>("recording-starting", () => cb()),
  onRecordingStarted: (cb: (p: RecordingStartedEvent) => void) =>
    on("recording-started", cb),
  onRecordingPaused: (cb: () => void) =>
    on<null>("recording-paused", () => cb()),
  onRecordingResumed: (cb: () => void) =>
    on<null>("recording-resumed", () => cb()),
  onRecordingStopping: (cb: (p: RecordingStoppingEvent) => void) =>
    on("recording-stopping", cb),
  onRecordingStopped: (cb: (p: RecordingStoppedEvent) => void) =>
    on("recording-stopped", cb),
  onRecordingError: (cb: (p: RecordingErrorEvent) => void) =>
    on("recording-error", cb),
  onTranscriptUpdate: (cb: (p: TranscriptUpdate) => void) =>
    on("transcript-update", cb),
  onTranscriptInterim: (cb: (p: TranscriptInterim) => void) =>
    on("transcript-interim", cb),
  onLiveTranscriptionStatus: (cb: (p: LiveTranscriptionStatus) => void) =>
    on("live-transcription-status", cb),
  onAudioLevels: (cb: (p: AudioLevels) => void) => on("audio-levels", cb),
  onFinalTranscriptStatus: (cb: (p: FinalTranscriptStatus) => void) =>
    on("final-transcript-status", cb),
  onMicDeviceSwitched: (cb: (p: MicDeviceSwitchedEvent) => void) =>
    on("mic-device-switched", cb),
  onMicUnavailable: (cb: () => void) => on<null>("mic-unavailable", () => cb()),
  onMicSwapFailed: (cb: (p: MicSwapFailedEvent) => void) =>
    on("mic-swap-failed", cb),
  onMicRecoveryExhausted: (cb: (p: MicRecoveryExhaustedEvent) => void) =>
    on("mic-recovery-exhausted", cb),
  onImportProgress: (cb: (p: ImportProgressEvent) => void) =>
    on("import-progress", cb),
  onImportComplete: (cb: (p: ImportCompleteEvent) => void) =>
    on("import-complete", cb),
  onImportError: (cb: (p: ImportErrorEvent) => void) => on("import-error", cb),
  onVoiceSettingsUpdated: (cb: (p: VoiceSettings) => void) =>
    on("voice-settings-updated", cb),
};

/** Error text from a rejected invoke (Rust returns plain strings). */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  return String(error);
}
