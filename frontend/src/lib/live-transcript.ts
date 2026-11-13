import type {
  AudioLevels,
  FinalTranscriptState,
  LiveChannel,
  LiveLine,
  LiveTranscriptionStatus,
  TranscriptInterim,
} from "@/types/voice";

export type InterimsByChannel = Partial<Record<LiveChannel, TranscriptInterim>>;

export type StatusByChannel = Partial<
  Record<LiveChannel, LiveTranscriptionStatus>
>;

/** Seconds of system silence after which the recording screen warns. */
export const SYSTEM_SILENCE_WARN_SECONDS = 30;

const CHANNEL_ORDER: LiveChannel[] = ["mic", "system", "mixed"];

/**
 * Adds one `transcript-update` line: a line from another meeting starts a fresh list, a repeated
 * `sequence_id` is ignored, and lines stay ordered by audio time.
 */
export function insertLiveLine(lines: LiveLine[], line: LiveLine): LiveLine[] {
  if (lines.length > 0 && lines[0].meeting_id !== line.meeting_id) {
    return [line];
  }

  if (lines.some((l) => l.sequence_id === line.sequence_id)) {
    return lines;
  }

  return [...lines, line].toSorted(
    (a, b) =>
      a.audio_start_time - b.audio_start_time || a.sequence_id - b.sequence_id
  );
}

/** `transcript-interim` replaces its channel's preview; empty text clears it. */
export function applyInterim(
  interims: InterimsByChannel,
  interim: TranscriptInterim
): InterimsByChannel {
  const next = { ...interims };

  if (interim.text) {
    next[interim.channel] = interim;
  } else {
    delete next[interim.channel];
  }

  return next;
}

/** `live-transcription-status`: latest state per channel; a closed session drops its pill. */
export function applyLiveStatus(
  statuses: StatusByChannel,
  status: LiveTranscriptionStatus
): StatusByChannel {
  const next = { ...statuses };

  if (status.state === "closed") {
    delete next[status.channel];
  } else {
    next[status.channel] = status;
  }

  return next;
}

/** Map values in a fixed channel order (mic, system, mixed). */
export function byChannel<T>(map: Partial<Record<LiveChannel, T>>): T[] {
  return CHANNEL_ORDER.flatMap((c) => {
    const value = map[c];

    return value === undefined ? [] : [value];
  });
}

export const CHANNEL_LABEL: Record<LiveChannel, string> = {
  mic: "Mic",
  mixed: "Mixed",
  system: "System",
};

export function isSystemSilent(
  levels: AudioLevels,
  recording: boolean,
  paused: boolean
): boolean {
  return (
    recording &&
    !paused &&
    levels.system_silent_seconds > SYSTEM_SILENCE_WARN_SECONDS
  );
}

const FINAL_IN_PROGRESS = new Set<FinalTranscriptState>([
  "queued",
  "uploading",
  "transcribing",
]);

export function isFinalInProgress(state: FinalTranscriptState | null): boolean {
  return state !== null && FINAL_IN_PROGRESS.has(state);
}

export const FINAL_STATE_TEXT: Record<FinalTranscriptState, string> = {
  done: "Final transcript ready",
  failed: "Final transcript failed",
  queued: "Final transcript queued…",
  transcribing: "Transcribing the recording…",
  uploading: "Uploading the recording…",
};
